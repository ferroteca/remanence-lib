// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The namespace node — [`StorageSpace`], the one type that carries file
//! verbs — and the P19 presentation contract beneath it.
//!
//! **File access lives here and nowhere else.** A medium bearing a
//! partition table and a `get_file` would be a category error in the
//! type rather than a refusal waiting to happen, so the medium exposes
//! no file access at all. The space is reached through the partition
//! that composes it: the vantage doors on
//! [`PartitionView`](crate::PartitionView) hand out this node, one door
//! per vantage and the same node behind both.
//!
//! A [`StorageSpace`] is a **view over its provider's state, never an
//! instance** (P23): its mutations project into the active layer, and it
//! stops answering when the medium beneath it leaves. Its kind — FAT,
//! HDOS — is data on the handle, not a type of its own.
//!
//! The rest of this module is the interface every file-bearing provider
//! presents through at the P19 seam, and the vocabulary they answer in.
//!
//! **There is no namespace layer here.** P19 makes the namespace a seam:
//! adapters *expose* a view and results *present* an interface. A ZIP
//! grammar, a FAT volume, a Commodore directory and a composed namespace
//! are real systems that already hold their own structure, and each of
//! them can present a view of it. Nothing in this module materializes a
//! namespace above them — there is no intermediate representation to
//! copy into, and so no second place a listing lives and nothing to
//! invalidate when the truth beneath changes.
//!
//! That is what keeps reads bounded (P27): a provider answers for the
//! directory it was asked about by reading that directory, rather than
//! building an item pool for fifty thousand files before the caller can
//! list one of them.
//!
//! Every presentation is a view *of* something — the lowest durable
//! layer the session has materialized, which is the source of truth and
//! which a presentation never is. That floor may be an archive's own
//! named-entry state, addressed CHS records, logical blocks, or timed
//! flux. Each item's [`FloorExtent`] hook says where it sits in the
//! floor's own addressing, and nothing is ever written through a
//! presentation.
//!
//! Two rules shape the vocabulary:
//!
//! - **Names and items are different things.** [`entries`] returns
//!   names reaching targets, so one item may be reached by several
//!   names, a flat filesystem is one root whose entries all reach
//!   leaves, and hierarchy is a directory whose entries reach
//!   directories. Listing order is evidence, carried as each entry's
//!   ordinal; nothing here re-sorts a source's listing.
//! - **The unclaimed remainder is itemized, never named.** In-force P19
//!   refuses to manufacture pseudo-files, so an [`ItemKind::OpaqueRegion`]
//!   is reached through the account rather than by path. That is what
//!   lets a view hold holes without lying in either direction: a
//!   presentation may be incomplete precisely because it is not the
//!   truth.
//!
//! Metadata follows the two-outcome rule the flux layer established one
//! seam down: a source fact either maps to a named field here, or is
//! retained as a [`DeclaredFact`] under its provider's namespace with
//! its source spelling and order intact. Nothing is normalized on the
//! way through — a timestamp keeps its source precision, epoch and zone
//! semantics, because the provider's namespace says what its value
//! means. [`EntryFact`] is that rule at the node's own surface.
//!
//! [`entries`]: FilesystemView::entries

// The providers that will present through the P19 contract below arrive
// with their own features; until then it has no non-test implementors.
#![allow(dead_code)]

use std::fs::File as SpoolFile;
use std::sync::Arc;

use crate::discovery::Discovery;
use crate::error::{Error, ErrorCategory, Result};
use crate::evidence::{DeclaredFact, Issue, Provenance};
use crate::fat::{FatEntry, FatEntryKind};
use crate::media::Medium;
use crate::report::{VolumeId, VolumeLabel};

/// Which rule of the storage-space seam's enumerated set a refusal broke
/// (P10).
///
/// The set answers *which rule did this input break* where the category
/// answers *how should the caller behave*, and it belongs to this seam
/// rather than to a second library-wide enum — [`crate::DosNameRule`] is
/// the same shape at the DOS 8.3 namespace, and
/// [`crate::PartitionRule`] the same shape one vantage out.
///
/// A [`StorageSpace`] carries two vantages, so the set covers both: the
/// absences are what a space answers when asked for a vantage it does not
/// have, which is trait presence stated as a refusal rather than a
/// failure to read something.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpaceRule {
    /// Nothing beneath this space bears a namespace at all. This is the
    /// honest absence P19 requires, not a failure to read one.
    NoNamespace,
    /// A namespace this release recognizes and does not read.
    RecognizedNotRead,
    /// A namespace this release reads and does not write.
    NotWritable,
    /// The space has no addressable vantage: it is a namespace composed
    /// over something that is not one addressed extent, so there is no
    /// position to read or write by. An archive's namespace is the case
    /// this release reaches.
    NotAddressable,
    /// A read or write named a position outside the space's own extent.
    /// The bound is the space's, not the medium's, which is the point of
    /// addressing within one.
    OutsideExtent,
}

impl SpaceRule {
    /// Every rule in the set.
    pub const ALL: [Self; 5] = [
        Self::NoNamespace,
        Self::RecognizedNotRead,
        Self::NotWritable,
        Self::NotAddressable,
        Self::OutsideExtent,
    ];

    /// The stable cross-language spelling of this rule, which is what a
    /// refusal carries.
    pub const fn as_str(self) -> crate::error::RuleIdentity {
        match self {
            Self::NoNamespace => "no-namespace",
            Self::RecognizedNotRead => "recognized-not-read",
            Self::NotWritable => "namespace-not-writable",
            Self::NotAddressable => "not-addressable",
            Self::OutsideExtent => "outside-extent",
        }
    }

    /// Reads a rule identity back into this set, for a caller branching
    /// on [`Error::rule`](crate::Error::rule). An identity from another
    /// seam's set is `None` rather than a nearest match.
    pub fn from_identity(identity: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|rule| rule.as_str() == identity)
    }
}

impl std::fmt::Display for SpaceRule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

fn refuse(rule: SpaceRule, reason: impl Into<String>) -> Error {
    let category = match rule {
        SpaceRule::RecognizedNotRead | SpaceRule::NotAddressable => ErrorCategory::Unsupported,
        SpaceRule::NoNamespace => ErrorCategory::NotFound,
        SpaceRule::NotWritable => ErrorCategory::ReadOnly,
        SpaceRule::OutsideExtent => ErrorCategory::Io,
    };
    Error::categorized_io(category, reason).broke_rule(rule.as_str())
}

/// What an entry names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    File,
    Directory,
}

impl EntryKind {
    /// The stable cross-language spelling of this kind.
    pub const fn name(self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Directory => "directory",
        }
    }
}

/// A fact the recognizing filesystem declares about an entry that this
/// vocabulary has no named field for, in that filesystem's own spelling.
///
/// This is the two-outcome rule at the node's surface: a source fact
/// either maps to a named field of [`Entry`], or is retained here with
/// its source spelling intact. Nothing is normalized on the way through,
/// so an HDOS catalog date keeps HDOS's reading of it and no caller has
/// to know which epoch it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryFact {
    /// The key as the recognizing filesystem spells it.
    pub key: String,
    /// The value as that filesystem reads it.
    pub value: String,
}

impl EntryFact {
    pub(crate) fn new(key: &str, value: impl Into<String>) -> Self {
        Self {
            key: key.to_owned(),
            value: value.into(),
        }
    }
}

/// One entry of a namespace, in the recognizing filesystem's own terms.
///
/// The name is as stored — nothing is truncated, transliterated, or
/// renamed to fit — and the listing keeps the source's own order, which
/// is evidence (U4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub kind: EntryKind,
    pub size_bytes: u64,
    /// What the recognizing filesystem declares beyond the fields above,
    /// in its own spelling and order. A filesystem whose format records
    /// none declares none.
    pub declared: Vec<EntryFact>,
}

impl Entry {
    fn from_fat(entry: &FatEntry) -> Self {
        Self {
            name: entry.name.clone(),
            kind: match entry.kind {
                FatEntryKind::File => EntryKind::File,
                FatEntryKind::Directory => EntryKind::Directory,
            },
            size_bytes: entry.size_bytes,
            declared: Vec::new(),
        }
    }
}

/// A namespace a medium bears directly, materialized bounded by the
/// adapter that recognized it.
///
/// A volume-backed filesystem needs none of this — it reads through the
/// composed volume — so this is the seam for the media whose namespace
/// *is* their content.
pub(crate) trait Catalog: std::fmt::Debug {
    fn entries(&self, path: &str) -> Result<Vec<Entry>>;
    fn stat(&self, path: &str) -> Result<Option<Entry>>;
    fn read_file(&self, path: &str) -> Result<Vec<u8>>;

    /// The label this namespace's own structures carry, where its format
    /// has such a field at all. A grammar that names no volume answers
    /// `None`, which is a different fact from a field that is present
    /// and blank.
    fn label(&self) -> Option<VolumeLabel> {
        None
    }

    /// What recognized this namespace, in human-readable terms (P4).
    ///
    /// A grammar recognized by the load that produced it — an archive's,
    /// which was claimed before there was a namespace to speak of —
    /// states nothing here, and the empty account is that fact rather
    /// than an omission.
    fn evidence(&self) -> Vec<String> {
        Vec::new()
    }

    /// Exactly `buf.len()` bytes at `offset` within one item (P27).
    ///
    /// The default is the honest one for a catalog whose medium is
    /// already resident within its declared bound — a slice of what is
    /// there, rather than a second pass. A provider whose items are
    /// spans of something larger overrides it and reads the span, so
    /// nothing is materialized to serve part of an item.
    fn read_file_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<()> {
        let bytes = self.read_file(path)?;
        let end = offset
            .checked_add(buf.len() as u64)
            .filter(|end| *end <= bytes.len() as u64)
            .ok_or_else(|| {
                Error::categorized_io(
                    ErrorCategory::NotFound,
                    format!(
                        "'{path}' holds {} bytes; the requested span at \
                         {offset} runs past it",
                        bytes.len()
                    ),
                )
            })?;
        buf.copy_from_slice(&bytes[offset as usize..end as usize]);
        Ok(())
    }
}

/// The HDOS catalog, read out of the medium it occupies.
///
/// HDOS has no subdirectories, so the namespace is one root of leaves:
/// any path but the root is refused where it is asked to hold names, and
/// answered as absent where it is asked for one.
#[derive(Debug)]
pub(crate) struct HdosCatalog {
    /// The medium's bytes, held for as long as the view is (P27): the
    /// format bounds an HDOS volume small, and reading one file out of
    /// it is a walk of the group table rather than a second pass over
    /// the medium.
    image: Vec<u8>,
    files: Vec<crate::hdos::HdosFile>,
}

/// The largest extent the HDOS reader will take whole (P27).
///
/// The bound is the adapter's own, and it is the whole of what bounds a
/// declared reading: a declaration names the adapter, and the adapter
/// says how much of an extent it is willing to hold.
const HDOS_BOUND: u64 = 8 * 1024 * 1024;

impl HdosCatalog {
    pub(crate) fn open(volume: &mut dyn crate::device::Device) -> Result<Self> {
        let length = volume.len();
        if length > HDOS_BOUND {
            return Err(Error::categorized_image(
                ErrorCategory::Unsupported,
                "hdos",
                format!(
                    "this extent is {length} bytes and the HDOS reader is \
                     bounded to {HDOS_BOUND}"
                ),
            ));
        }
        let mut image = vec![0u8; length as usize];
        volume.read_at(0, &mut image)?;
        let files = crate::hdos::list_hdos_files(&image)?;
        Ok(Self { image, files })
    }

    fn entry(file: &crate::hdos::HdosFile) -> Entry {
        Entry {
            name: file.display_name(),
            kind: EntryKind::File,
            size_bytes: file.size_bytes(),
            declared: vec![
                EntryFact::new("size-sectors", file.size_sectors.to_string()),
                EntryFact::new("modified-date", file.modified_date_string()),
                EntryFact::new("modified-date-raw", file.modified_date.to_string()),
                EntryFact::new("flags", file.flags_string()),
                EntryFact::new("flags-raw", file.flags.to_string()),
            ],
        }
    }
}

impl Catalog for HdosCatalog {
    fn entries(&self, path: &str) -> Result<Vec<Entry>> {
        if !path_is_root(path) {
            return Err(Error::categorized_image(
                ErrorCategory::NotDirectory,
                "hdos",
                format!("'{path}' holds no names; the HDOS catalog is flat"),
            ));
        }
        Ok(self.files.iter().map(Self::entry).collect())
    }

    fn stat(&self, path: &str) -> Result<Option<Entry>> {
        Ok(self
            .files
            .iter()
            .find(|file| file.display_name().eq_ignore_ascii_case(path))
            .map(Self::entry))
    }

    fn read_file(&self, path: &str) -> Result<Vec<u8>> {
        crate::hdos::read_hdos_file(&self.image, path)
    }
}

fn path_is_root(path: &str) -> bool {
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
    /// loaded into a device of its own — in a machine of its own where a
    /// machine is being reconstructed, because the host's archive was
    /// never part of the machine whose disk it holds. The claim is the
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

/// A provider's own identity for one item.
///
/// The value belongs to the provider that issued it — a directory
/// entry's location, a central-directory index, an inode — and means
/// nothing to anyone else. Nothing in this module assigns identities,
/// because nothing here holds a pool to index into. Callers pass a ref
/// back to the provider that gave it to them and never interpret it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct ItemRef(pub(crate) u64);

/// The encoding a provider claims for a recorded name.
///
/// The set grows additively as providers are admitted, exactly as the
/// flux layer's named homes do: a provider whose encoding has no
/// variant here keeps the name's bytes and does not claim an encoding
/// it has not implemented.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NameEncoding {
    Ascii,
    Utf8,
    Utf16Le,
    /// Commodore PETSCII, in the provider's claimed variant.
    Petscii,
    /// A DOS/Windows OEM code page, by its numeric identity.
    OemCodepage(u16),
}

/// How faithfully a decoded presentation represents the recorded bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NameConversion {
    /// Every byte mapped, and the presentation is reversible.
    Exact,
    /// Bytes the claimed encoding does not cover were substituted. The
    /// recorded bytes remain authoritative; this names what happened.
    Lossy { detail: String },
}

/// One name a source recorded, kept as bytes first.
///
/// The bytes are what the source stored and are never rewritten. The
/// decoded presentation sits beside them with the provenance of its
/// conversion, never in place of them: a ZIP name is UTF-8 only when
/// the grammar's own flag says so, and irregular names — trailing
/// spaces, shift characters, bytes outside the claimed encoding — stay
/// as recorded and are reported as issues rather than repaired.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RecordedName {
    pub(crate) bytes: Vec<u8>,
    pub(crate) encoding: NameEncoding,
    pub(crate) decoded: String,
    pub(crate) conversion: NameConversion,
}

impl RecordedName {
    pub(crate) fn new(
        bytes: impl Into<Vec<u8>>,
        encoding: NameEncoding,
        decoded: impl Into<String>,
        conversion: NameConversion,
    ) -> Self {
        Self {
            bytes: bytes.into(),
            encoding,
            decoded: decoded.into(),
            conversion,
        }
    }

    /// A name whose source bytes are already UTF-8, decoded exactly.
    pub(crate) fn utf8(name: impl Into<String>) -> Self {
        let decoded = name.into();
        Self {
            bytes: decoded.as_bytes().to_vec(),
            encoding: NameEncoding::Utf8,
            decoded,
            conversion: NameConversion::Exact,
        }
    }
}

/// One name in one directory, reaching one item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NameEntry {
    pub(crate) name: RecordedName,
    pub(crate) target: ItemRef,
    /// The entry's position in the source's own listing order.
    pub(crate) ordinal: u64,
}

/// What an item is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ItemKind {
    /// A namespace level, whose entries name other items. It claims
    /// namespace structure, not allocation in the floor.
    Directory,
    /// An item with extractable content, reached through its hook.
    File,
    /// The itemized remainder of the account: an extent of the floor
    /// this interpretation does not claim. It is never named, never a
    /// verdict about what the extent holds, and never a pseudo-file.
    OpaqueRegion,
}

impl ItemKind {
    /// The kind's stable spelling, for diagnostics.
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Directory => "directory",
            Self::File => "file",
            Self::OpaqueRegion => "opaque region",
        }
    }
}

/// What a provider's size claim is actually a claim *about*.
///
/// These are different claims, not one number: an exactly stored byte
/// count, a size expressed in allocation units, and a rounded count of
/// fixed-length records each say something different about the bytes a
/// read will produce.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SizeBasis {
    /// The source stores the byte length itself.
    Exact,
    /// The source states a count of allocation units only.
    AllocationUnits { unit_bytes: u64, units: u64 },
    /// The source states a count of fixed-length records, so the byte
    /// length is rounded to the record boundary (CP/M's 128-byte
    /// records).
    RecordCount { record_bytes: u64, records: u64 },
}

/// The byte size a provider claims for a file item, with its basis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SizeClaim {
    pub(crate) bytes: u64,
    pub(crate) basis: SizeBasis,
}

impl SizeClaim {
    pub(crate) const fn exact(bytes: u64) -> Self {
        Self {
            bytes,
            basis: SizeBasis::Exact,
        }
    }
}

/// Where a file item's bytes come from, bounded (P27).
///
/// This is a descriptor, never the bytes: a presentation is a map and
/// not a pipe, and reading resolves through the provider that owns the
/// floor.
pub(crate) enum ContentSource {
    /// A span of the claimed source artifact, read in place.
    InPlace { offset: u64, length: u64 },
    /// Decoded once into private session storage.
    Spooled { spool: Arc<SpoolFile>, length: u64 },
    /// Assembled from extents of the floor, in the order given.
    Floor { extents: Vec<FloorExtent> },
    /// The item holds no bytes at all.
    Empty,
}

impl std::fmt::Debug for ContentSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InPlace { offset, length } => f
                .debug_struct("InPlace")
                .field("offset", offset)
                .field("length", length)
                .finish(),
            Self::Spooled { length, .. } => {
                f.debug_struct("Spooled").field("length", length).finish()
            }
            Self::Floor { extents } => f
                .debug_struct("Floor")
                .field("extents", &extents.len())
                .finish(),
            Self::Empty => f.write_str("Empty"),
        }
    }
}

/// What one addressable unit of the floor *is*.
///
/// A presentation has exactly one floor, so hooks and the account share
/// one unit ordering and totality is checkable. The descriptor keeps
/// the vocabulary's own spelling recoverable; extents index units in
/// it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FloorAddressing {
    /// Bytes of a serialized artifact — an archive's own named-entry
    /// state is presented over its file this way.
    Bytes,
    /// Geometry-opaque logical blocks numbered from zero.
    Blocks { bytes_per_block: u32 },
    /// Records addressed by cylinder, head and sector under a declared
    /// geometry, numbered in CHS order.
    Chs {
        heads: u32,
        sectors_per_track: u32,
        bytes_per_sector: u32,
    },
    /// Track-relative angular flux regions: units run through each
    /// track's revolution in turn.
    Flux { ticks_per_revolution: u64 },
}

impl FloorAddressing {
    /// The unit's stable spelling, for diagnostics.
    pub(crate) const fn unit(self) -> &'static str {
        match self {
            Self::Bytes => "byte",
            Self::Blocks { .. } => "block",
            Self::Chs { .. } => "record",
            Self::Flux { .. } => "flux unit",
        }
    }
}

/// The lowest durable layer the session has materialized: the truth a
/// presentation is a view of, and never is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Floor {
    pub(crate) addressing: FloorAddressing,
    /// The addressable extent the account must cover in full.
    pub(crate) total_units: u64,
}

/// A half-open run of addressable units in the floor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FloorExtent {
    pub(crate) start: u64,
    pub(crate) count: u64,
}

impl FloorExtent {
    pub(crate) const fn new(start: u64, count: u64) -> Self {
        Self { start, count }
    }

    /// The first unit past the extent, or `None` if the run overflows.
    pub(crate) fn end(&self) -> Option<u64> {
        self.start.checked_add(self.count)
    }
}

/// A source structure retained verbatim because the vocabulary has no
/// named home for it yet.
///
/// Retaining it is the first of the two outcomes, not the permanent
/// one: a fact stays foreign only until a later revision gives it a
/// named field, which is what stops this becoming a blind spot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ForeignRecord {
    pub(crate) namespace: &'static str,
    pub(crate) type_id: String,
    pub(crate) version: Option<u32>,
    pub(crate) ordinal: u64,
    /// Where it sat in the floor, where the provider can say.
    pub(crate) source_range: Option<FloorExtent>,
    pub(crate) payload: Vec<u8>,
    /// Whatever the provider could safely decode, if anything.
    pub(crate) decoded_summary: Vec<DeclaredFact>,
}

/// Everything a provider claims about one item.
#[derive(Debug)]
pub(crate) struct ItemFacts {
    pub(crate) kind: ItemKind,
    /// Present for a file item; a claim, with what it is a claim about.
    pub(crate) size: Option<SizeClaim>,
    /// Where the item sits in the floor, in the floor's addressing.
    pub(crate) hook: Vec<FloorExtent>,
    pub(crate) facts: Vec<DeclaredFact>,
    pub(crate) issues: Vec<Issue>,
    pub(crate) provenance: Provenance,
}

impl ItemFacts {
    pub(crate) fn new(kind: ItemKind, provenance: Provenance) -> Self {
        Self {
            kind,
            size: None,
            hook: Vec::new(),
            facts: Vec::new(),
            issues: Vec::new(),
            provenance,
        }
    }
}

/// The view a file-bearing system presents at the P19 seam.
///
/// Implementors are the real systems themselves — an archive grammar, a
/// filesystem adapter, a namespace composer — answering about their own
/// structure. The shape is navigational rather than wholesale so that
/// answering one question costs one question's worth of reading (P27);
/// [`account`] is the deliberate exception, being a whole-artifact
/// report produced only when asked for.
///
/// [`account`]: FilesystemView::account
pub(crate) trait FilesystemView {
    /// The provider namespace, which attributes its refusals.
    fn source(&self) -> &'static str;

    /// The floor this is a view of.
    fn floor(&self) -> Floor;

    /// The namespace's roots, each a directory. Several are ordinary:
    /// a composed namespace may have one per drive letter or mount.
    fn roots(&self) -> Result<Vec<ItemRef>>;

    /// The entries of one directory, in the source's own order.
    ///
    /// An entry never targets an opaque region: the namespace lists
    /// only what the source names.
    fn entries(&self, directory: ItemRef) -> Result<Vec<NameEntry>>;

    /// What the provider claims about one item.
    fn item(&self, item: ItemRef) -> Result<ItemFacts>;

    /// Where one file item's bytes come from, bounded.
    fn content(&self, item: ItemRef) -> Result<ContentSource>;

    /// The total account of the floor, computed on request.
    fn account(&self) -> Result<CoverageAccount>;
}

/// The refusal for an operation that needs a directory.
pub(crate) fn not_a_directory(source: &'static str, kind: ItemKind) -> Error {
    Error::invalid_image(source, format!("a {} holds no names", kind.as_str()))
}

/// The refusal for an item ref this provider did not issue.
pub(crate) fn no_such_item(source: &'static str, item: ItemRef) -> Error {
    Error::invalid_image(source, format!("this view holds no item {}", item.0))
}

/// The refusal for content asked of something that holds none.
pub(crate) fn not_a_file(source: &'static str, kind: ItemKind) -> Error {
    Error::invalid_image(
        source,
        format!("a {} has no extractable content", kind.as_str()),
    )
}

/// How one addressable unit of the floor is accounted for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CoverageClass {
    /// The data hook of an item the namespace names.
    ItemData(ItemRef),
    /// Structures the interpretation claims for itself — directory
    /// records, allocation metadata, boot and reserved areas, an
    /// archive's local headers and central directory. Deleted-but-
    /// present entries are accounted here, inside the structures they
    /// occupy; itemizing them would be a recovery claim.
    Structures,
    /// Space the allocation metadata claims is free. This records that
    /// claim and nothing else: not a verdict that the extent is empty,
    /// disposable, or safe to reuse.
    ClaimedFree,
    /// An extent the interpretation does not claim, itemized as the
    /// opaque region it names.
    Opaque(ItemRef),
}

impl CoverageClass {
    /// The class's stable spelling, for diagnostics.
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::ItemData(_) => "item data",
            Self::Structures => "interpretation structures",
            Self::ClaimedFree => "claimed-free space",
            Self::Opaque(_) => "opaque region",
        }
    }
}

/// One classified run of the floor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CoverageRegion {
    pub(crate) extent: FloorExtent,
    pub(crate) class: CoverageClass,
    /// Why the class was assigned, in human-readable terms (P4).
    pub(crate) evidence: Vec<String>,
}

/// A total, exclusive account of the floor.
///
/// Totality is true by construction rather than by assertion: the
/// provider claims what its interpretation covers and whatever remains
/// becomes opaque regions, so the regions are ordered by position and
/// together span exactly `[0, floor.total_units)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CoverageAccount {
    pub(crate) floor: Floor,
    pub(crate) regions: Vec<CoverageRegion>,
    /// The first ref assigned to an opaque region; refs run upward from
    /// it in position order.
    opaque_base: u64,
}

impl CoverageAccount {
    /// The regions whose class matches, in position order.
    pub(crate) fn regions_of(
        &self,
        matching: impl Fn(&CoverageClass) -> bool,
    ) -> impl Iterator<Item = &CoverageRegion> {
        self.regions
            .iter()
            .filter(move |region| matching(&region.class))
    }

    /// The number of units accounted to matching classes.
    pub(crate) fn units_of(&self, matching: impl Fn(&CoverageClass) -> bool) -> u64 {
        self.regions_of(matching)
            .map(|region| region.extent.count)
            .sum()
    }

    /// Whether `item` names one of this account's opaque regions.
    pub(crate) fn is_opaque(&self, item: ItemRef) -> bool {
        self.opaque_region(item).is_some()
    }

    /// The opaque region `item` names, if it names one.
    pub(crate) fn opaque_region(&self, item: ItemRef) -> Option<&CoverageRegion> {
        self.regions
            .iter()
            .find(|region| region.class == CoverageClass::Opaque(item))
    }

    /// The facts of an opaque region, so a provider can answer
    /// [`FilesystemView::item`] for one without tracking it itself.
    pub(crate) fn opaque_facts(&self, item: ItemRef, source: &'static str) -> Option<ItemFacts> {
        let region = self.opaque_region(item)?;
        let mut facts = ItemFacts::new(
            ItemKind::OpaqueRegion,
            Provenance::new(source).note("the interpretation claims no structure over this extent"),
        );
        facts.hook = vec![region.extent];
        Some(facts)
    }
}

/// Builds a [`CoverageAccount`] by claiming what an interpretation
/// covers and deriving the rest.
///
/// Claims are checked as they arrive (P6): an extent outside the floor,
/// an empty run, or an overlap with an existing claim is refused there
/// and then, naming both sides, rather than producing an account that
/// quietly contradicts itself.
pub(crate) struct CoverageBuilder {
    source: &'static str,
    floor: Floor,
    opaque_base: u64,
    /// Kept ordered by `extent.start`, and disjoint by construction.
    claimed: Vec<CoverageRegion>,
}

impl CoverageBuilder {
    /// Begins an account of `floor`.
    ///
    /// `opaque_base` is the first [`ItemRef`] value the builder may
    /// assign to a derived opaque region; the provider chooses a range
    /// that cannot collide with the refs it issues itself.
    pub(crate) fn new(source: &'static str, floor: Floor, opaque_base: u64) -> Self {
        Self {
            source,
            floor,
            opaque_base,
            claimed: Vec::new(),
        }
    }

    /// Claims `extent` for `class`, with the evidence for that reading.
    pub(crate) fn claim(
        &mut self,
        extent: FloorExtent,
        class: CoverageClass,
        evidence: Vec<String>,
    ) -> Result<()> {
        let unit = self.floor.addressing.unit();
        if extent.count == 0 {
            return Err(self.refuse(format!(
                "coverage claim at {unit} {} spans nothing; an item that occupies \
                 no {unit}s carries no extent at all",
                extent.start
            )));
        }
        let end = extent.end().ok_or_else(|| {
            self.refuse(format!(
                "coverage claim at {unit} {} for {} {unit}s runs past the end of \
                 the unit space",
                extent.start, extent.count
            ))
        })?;
        if end > self.floor.total_units {
            return Err(self.refuse(format!(
                "coverage claim covers {unit}s {}..{end}, past the floor's {} {unit}s",
                extent.start, self.floor.total_units
            )));
        }

        let position = self
            .claimed
            .partition_point(|region| region.extent.start < extent.start);
        if let Some(previous) = position.checked_sub(1).and_then(|at| self.claimed.get(at))
            && previous
                .extent
                .end()
                .is_some_and(|prior_end| prior_end > extent.start)
        {
            return Err(self.overlap(previous, extent));
        }
        if let Some(next) = self.claimed.get(position)
            && next.extent.start < end
        {
            return Err(self.overlap(next, extent));
        }

        self.claimed.insert(
            position,
            CoverageRegion {
                extent,
                class,
                evidence,
            },
        );
        Ok(())
    }

    /// Derives the opaque remainder and returns the finished account.
    ///
    /// Every gap between claims becomes one opaque region, so the
    /// account spans the floor exactly.
    pub(crate) fn finish(self) -> CoverageAccount {
        let unit = self.floor.addressing.unit();
        let mut regions: Vec<CoverageRegion> = Vec::with_capacity(self.claimed.len() + 1);
        let mut opaque_next = self.opaque_base;
        let mut position = 0_u64;

        let mut derive = |extent: FloorExtent, regions: &mut Vec<CoverageRegion>| {
            let item = ItemRef(opaque_next);
            opaque_next += 1;
            regions.push(CoverageRegion {
                extent,
                class: CoverageClass::Opaque(item),
                evidence: vec![format!(
                    "{unit}s {}..{} are covered by no claim of this interpretation",
                    extent.start,
                    extent.end().unwrap_or(u64::MAX)
                )],
            });
        };

        for region in self.claimed {
            if region.extent.start > position {
                derive(
                    FloorExtent::new(position, region.extent.start - position),
                    &mut regions,
                );
            }
            position = region.extent.end().expect("claims were bounds-checked");
            regions.push(region);
        }
        if position < self.floor.total_units {
            derive(
                FloorExtent::new(position, self.floor.total_units - position),
                &mut regions,
            );
        }

        CoverageAccount {
            floor: self.floor,
            regions,
            opaque_base: self.opaque_base,
        }
    }

    fn overlap(&self, existing: &CoverageRegion, incoming: FloorExtent) -> Error {
        let unit = self.floor.addressing.unit();
        self.refuse(format!(
            "coverage claim covering {unit}s {}..{} overlaps the {} already claimed \
             at {unit}s {}..{}",
            incoming.start,
            incoming.end().unwrap_or(u64::MAX),
            existing.class.as_str(),
            existing.extent.start,
            existing.extent.end().unwrap_or(u64::MAX),
        ))
    }

    fn refuse(&self, reason: impl Into<String>) -> Error {
        Error::invalid_image(self.source, reason)
    }
}

/// Checks one presentation against the contract's invariants.
///
/// This is a conformance harness for tests and for a new provider's own
/// verification, not a runtime path: it walks the whole namespace, which
/// is exactly what the interface exists to avoid doing during ordinary
/// use. It proves what the interface promises and a provider could
/// otherwise get wrong on its own: that the account is total and
/// exclusive over the declared floor, and that no entry names an opaque
/// region.
pub(crate) fn check_conformance(view: &dyn FilesystemView) -> Result<()> {
    let source = view.source();
    let floor = view.floor();
    let account = view.account()?;

    if account.floor != floor {
        return Err(Error::invalid_image(
            source,
            "the account describes a different floor than the view declares",
        ));
    }

    let unit = floor.addressing.unit();
    let mut position = 0_u64;
    for region in &account.regions {
        if region.extent.start != position {
            return Err(Error::invalid_image(
                source,
                format!(
                    "the account skips or repeats at {unit} {position}: the next \
                     region starts at {}",
                    region.extent.start
                ),
            ));
        }
        position = region.extent.end().ok_or_else(|| {
            Error::invalid_image(
                source,
                format!("an account region overflows at {unit} {position}"),
            )
        })?;
    }
    if position != floor.total_units {
        return Err(Error::invalid_image(
            source,
            format!(
                "the account covers {position} of the floor's {} {unit}s",
                floor.total_units
            ),
        ));
    }

    let mut pending = view.roots()?;
    let mut seen: Vec<ItemRef> = Vec::new();
    while let Some(item) = pending.pop() {
        if seen.contains(&item) {
            continue;
        }
        seen.push(item);
        let facts = view.item(item)?;
        if facts.kind != ItemKind::Directory {
            continue;
        }
        for entry in view.entries(item)? {
            if account.is_opaque(entry.target) {
                return Err(Error::invalid_image(
                    source,
                    format!(
                        "the entry '{}' names an opaque region; the namespace lists \
                         only what the source names",
                        entry.name.decoded
                    ),
                ));
            }
            pending.push(entry.target);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCategory;

    const PROVIDER: &str = "test-provider";
    const OPAQUE_BASE: u64 = 1 << 32;

    /// A synthetic system that presents a view of its own structure —
    /// the contract's test surface, standing in for a real grammar.
    struct SyntheticProvider {
        floor: Floor,
        /// `(kind, size, hook, entries)` by ref value.
        items: Vec<(
            ItemKind,
            Option<SizeClaim>,
            Vec<FloorExtent>,
            Vec<NameEntry>,
        )>,
        roots: Vec<ItemRef>,
        claims: Vec<(FloorExtent, CoverageClass)>,
    }

    impl SyntheticProvider {
        fn new(total_units: u64) -> Self {
            Self {
                floor: Floor {
                    addressing: FloorAddressing::Blocks {
                        bytes_per_block: 256,
                    },
                    total_units,
                },
                items: Vec::new(),
                roots: Vec::new(),
                claims: Vec::new(),
            }
        }

        fn bytes_floor(total_units: u64) -> Self {
            let mut provider = Self::new(total_units);
            provider.floor = Floor {
                addressing: FloorAddressing::Bytes,
                total_units,
            };
            provider
        }

        fn add(&mut self, kind: ItemKind, hook: Vec<FloorExtent>) -> ItemRef {
            self.items.push((kind, None, hook, Vec::new()));
            ItemRef(self.items.len() as u64 - 1)
        }

        fn add_file(&mut self, size: SizeClaim, hook: Vec<FloorExtent>) -> ItemRef {
            let item = self.add(ItemKind::File, hook);
            self.items[item.0 as usize].1 = Some(size);
            item
        }

        fn link(&mut self, parent: ItemRef, name: RecordedName, target: ItemRef) {
            let ordinal = self.items[parent.0 as usize].3.len() as u64;
            self.items[parent.0 as usize].3.push(NameEntry {
                name,
                target,
                ordinal,
            });
        }

        fn claim(&mut self, extent: FloorExtent, class: CoverageClass) {
            self.claims.push((extent, class));
        }

        fn entry_names(&self, directory: ItemRef) -> Vec<String> {
            self.entries(directory)
                .expect("the directory exists")
                .into_iter()
                .map(|entry| entry.name.decoded)
                .collect()
        }
    }

    impl FilesystemView for SyntheticProvider {
        fn source(&self) -> &'static str {
            PROVIDER
        }

        fn floor(&self) -> Floor {
            self.floor
        }

        fn roots(&self) -> Result<Vec<ItemRef>> {
            Ok(self.roots.clone())
        }

        fn entries(&self, directory: ItemRef) -> Result<Vec<NameEntry>> {
            let item = self
                .items
                .get(directory.0 as usize)
                .ok_or_else(|| no_such_item(PROVIDER, directory))?;
            if item.0 != ItemKind::Directory {
                return Err(not_a_directory(PROVIDER, item.0));
            }
            Ok(item.3.clone())
        }

        fn item(&self, item: ItemRef) -> Result<ItemFacts> {
            if let Some(facts) = self.account()?.opaque_facts(item, PROVIDER) {
                return Ok(facts);
            }
            let held = self
                .items
                .get(item.0 as usize)
                .ok_or_else(|| no_such_item(PROVIDER, item))?;
            let mut facts = ItemFacts::new(held.0, Provenance::new(PROVIDER));
            facts.size = held.1;
            facts.hook = held.2.clone();
            Ok(facts)
        }

        fn content(&self, item: ItemRef) -> Result<ContentSource> {
            let held = self
                .items
                .get(item.0 as usize)
                .ok_or_else(|| no_such_item(PROVIDER, item))?;
            if held.0 != ItemKind::File {
                return Err(not_a_file(PROVIDER, held.0));
            }
            Ok(ContentSource::Floor {
                extents: held.2.clone(),
            })
        }

        fn account(&self) -> Result<CoverageAccount> {
            let mut builder = CoverageBuilder::new(PROVIDER, self.floor, OPAQUE_BASE);
            for (extent, class) in &self.claims {
                builder.claim(*extent, *class, Vec::new())?;
            }
            Ok(builder.finish())
        }
    }

    #[test]
    fn one_item_may_be_reached_by_several_names() {
        let mut provider = SyntheticProvider::new(16);
        let root = provider.add(ItemKind::Directory, Vec::new());
        provider.roots.push(root);
        let file = provider.add_file(SizeClaim::exact(12), vec![FloorExtent::new(4, 1)]);

        provider.link(root, RecordedName::utf8("README"), file);
        provider.link(root, RecordedName::utf8("readme.txt"), file);

        let entries = provider.entries(root).expect("the root is a directory");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].target, entries[1].target);
        check_conformance(&provider).expect("the contract holds");
    }

    #[test]
    fn source_listing_order_is_preserved_as_evidence() {
        let mut provider = SyntheticProvider::new(16);
        let root = provider.add(ItemKind::Directory, Vec::new());
        provider.roots.push(root);
        for name in ["ZEBRA", "alpha", "Middle"] {
            let file = provider.add_file(SizeClaim::exact(0), Vec::new());
            provider.link(root, RecordedName::utf8(name), file);
        }

        assert_eq!(provider.entry_names(root), ["ZEBRA", "alpha", "Middle"]);
        let ordinals: Vec<u64> = provider
            .entries(root)
            .expect("the root is a directory")
            .iter()
            .map(|entry| entry.ordinal)
            .collect();
        assert_eq!(ordinals, [0, 1, 2]);
    }

    #[test]
    fn a_name_keeps_its_recorded_bytes_and_claimed_encoding() {
        let mut provider = SyntheticProvider::new(16);
        let root = provider.add(ItemKind::Directory, Vec::new());
        let file = provider.add_file(SizeClaim::exact(2), Vec::new());
        // A CBM name padded with shifted spaces: the bytes are the
        // evidence, and the presentation never replaces them.
        provider.link(
            root,
            RecordedName::new(
                vec![0x50, 0x49, 0x43, 0xa0, 0xa0],
                NameEncoding::Petscii,
                "PIC",
                NameConversion::Lossy {
                    detail: "two trailing shifted spaces are not presentable".to_owned(),
                },
            ),
            file,
        );

        let entries = provider.entries(root).expect("the root is a directory");
        let name = &entries[0].name;
        assert_eq!(name.bytes, [0x50, 0x49, 0x43, 0xa0, 0xa0]);
        assert_eq!(name.encoding, NameEncoding::Petscii);
        assert_eq!(name.decoded, "PIC");
        assert!(matches!(name.conversion, NameConversion::Lossy { .. }));
    }

    #[test]
    fn the_account_is_total_because_the_remainder_is_derived() {
        let mut provider = SyntheticProvider::new(100);
        let file = provider.add_file(SizeClaim::exact(2560), vec![FloorExtent::new(10, 10)]);
        provider.claim(FloorExtent::new(0, 4), CoverageClass::Structures);
        provider.claim(FloorExtent::new(10, 10), CoverageClass::ItemData(file));
        provider.claim(FloorExtent::new(20, 60), CoverageClass::ClaimedFree);

        let account = provider.account().expect("the account computes");
        let mut position = 0;
        for region in &account.regions {
            assert_eq!(region.extent.start, position);
            position = region.extent.end().expect("bounded");
        }
        assert_eq!(position, 100);
        assert_eq!(
            account.units_of(|class| matches!(class, CoverageClass::Opaque(_))),
            26,
            "blocks 4..10 and 80..100 belong to no claim"
        );
        check_conformance(&provider).expect("the contract holds");
    }

    #[test]
    fn an_archive_floor_accounts_its_unclaimed_bytes_the_same_way() {
        // A self-extractor stub ahead of the first local header is an
        // opaque region exactly as a protection track is.
        let mut provider = SyntheticProvider::bytes_floor(4096);
        let member = provider.add_file(SizeClaim::exact(512), vec![FloorExtent::new(2048, 512)]);
        provider.claim(FloorExtent::new(2048, 512), CoverageClass::ItemData(member));
        provider.claim(FloorExtent::new(2560, 1536), CoverageClass::Structures);

        let account = provider.account().expect("the account computes");
        let opaque: Vec<&CoverageRegion> = account
            .regions_of(|class| matches!(class, CoverageClass::Opaque(_)))
            .collect();
        assert_eq!(opaque.len(), 1);
        assert_eq!(opaque[0].extent, FloorExtent::new(0, 2048));
        assert!(
            opaque[0].evidence[0].contains("bytes 0..2048"),
            "{:?}",
            opaque[0]
        );
    }

    #[test]
    fn an_opaque_region_is_never_named_in_the_namespace() {
        let mut provider = SyntheticProvider::new(16);
        let root = provider.add(ItemKind::Directory, Vec::new());
        provider.roots.push(root);
        provider.claim(FloorExtent::new(0, 4), CoverageClass::Structures);

        let account = provider.account().expect("the account computes");
        let opaque = account
            .regions_of(|class| matches!(class, CoverageClass::Opaque(_)))
            .next()
            .expect("the remainder was itemized");
        let CoverageClass::Opaque(opaque_ref) = opaque.class else {
            panic!("the class is opaque");
        };

        // It is a real item, reachable through the account...
        let facts = provider
            .item(opaque_ref)
            .expect("the account answers for it");
        assert_eq!(facts.kind, ItemKind::OpaqueRegion);
        assert_eq!(facts.hook, vec![FloorExtent::new(4, 12)]);
        check_conformance(&provider).expect("nothing names it");

        // ...and naming it is what the contract catches.
        provider.link(root, RecordedName::utf8("PROTECTION"), opaque_ref);
        let error = check_conformance(&provider).expect_err("the pseudo-file rule holds");
        assert_eq!(error.category(), ErrorCategory::InvalidImage);
        assert!(
            error
                .to_string()
                .contains("lists only what the source names"),
            "{error}"
        );
    }

    #[test]
    fn overlapping_claims_are_refused_naming_both_sides() {
        let mut builder = CoverageBuilder::new(
            PROVIDER,
            Floor {
                addressing: FloorAddressing::Blocks {
                    bytes_per_block: 256,
                },
                total_units: 50,
            },
            OPAQUE_BASE,
        );
        builder
            .claim(
                FloorExtent::new(10, 10),
                CoverageClass::Structures,
                Vec::new(),
            )
            .expect("in bounds");

        let error = builder
            .claim(
                FloorExtent::new(15, 10),
                CoverageClass::ClaimedFree,
                Vec::new(),
            )
            .expect_err("the extents overlap");
        assert_eq!(error.category(), ErrorCategory::InvalidImage);
        let message = error.to_string();
        assert!(message.contains("blocks 15..25"), "{message}");
        assert!(message.contains("interpretation structures"), "{message}");
        assert!(message.contains("blocks 10..20"), "{message}");
    }

    #[test]
    fn a_claim_past_the_floor_is_refused_in_the_floors_own_units() {
        let mut builder = CoverageBuilder::new(
            PROVIDER,
            Floor {
                addressing: FloorAddressing::Flux {
                    ticks_per_revolution: 3_200_000,
                },
                total_units: 32,
            },
            OPAQUE_BASE,
        );
        let error = builder
            .claim(
                FloorExtent::new(30, 8),
                CoverageClass::ClaimedFree,
                Vec::new(),
            )
            .expect_err("the claim runs past the end");
        assert!(
            error.to_string().contains("past the floor's 32 flux units"),
            "{error}"
        );
        assert!(
            builder
                .claim(
                    FloorExtent::new(0, 0),
                    CoverageClass::ClaimedFree,
                    Vec::new()
                )
                .is_err(),
            "an extent spanning nothing claims nothing"
        );
    }

    #[test]
    fn conformance_catches_an_account_that_does_not_cover_its_floor() {
        struct ShortAccount;

        impl FilesystemView for ShortAccount {
            fn source(&self) -> &'static str {
                PROVIDER
            }
            fn floor(&self) -> Floor {
                Floor {
                    addressing: FloorAddressing::Bytes,
                    total_units: 64,
                }
            }
            fn roots(&self) -> Result<Vec<ItemRef>> {
                Ok(Vec::new())
            }
            fn entries(&self, directory: ItemRef) -> Result<Vec<NameEntry>> {
                Err(no_such_item(PROVIDER, directory))
            }
            fn item(&self, item: ItemRef) -> Result<ItemFacts> {
                Err(no_such_item(PROVIDER, item))
            }
            fn content(&self, item: ItemRef) -> Result<ContentSource> {
                Err(no_such_item(PROVIDER, item))
            }
            fn account(&self) -> Result<CoverageAccount> {
                // Declares 64 bytes, accounts for 32.
                let mut builder = CoverageBuilder::new(
                    PROVIDER,
                    Floor {
                        addressing: FloorAddressing::Bytes,
                        total_units: 32,
                    },
                    OPAQUE_BASE,
                );
                builder.claim(
                    FloorExtent::new(0, 32),
                    CoverageClass::Structures,
                    Vec::new(),
                )?;
                Ok(builder.finish())
            }
        }

        let error = check_conformance(&ShortAccount).expect_err("the floors disagree");
        assert!(error.to_string().contains("different floor"), "{error}");
    }

    #[test]
    fn content_stays_a_bounded_descriptor() {
        let mut provider = SyntheticProvider::bytes_floor(96 * 1024 * 1024 * 1024);
        let huge = provider.add_file(
            SizeClaim {
                bytes: 96 * 1024 * 1024 * 1024,
                basis: SizeBasis::AllocationUnits {
                    unit_bytes: 4096,
                    units: 25_165_824,
                },
            },
            vec![FloorExtent::new(512, 96 * 1024 * 1024 * 1024 - 512)],
        );

        // Ninety-six gibibytes described without holding any of it.
        let content = provider.content(huge).expect("the item is a file");
        assert!(matches!(content, ContentSource::Floor { .. }));
        let facts = provider.item(huge).expect("the item exists");
        assert_eq!(facts.size.expect("a file claims a size").bytes, 96 << 30);
    }

    #[test]
    fn a_size_claim_records_what_it_is_a_claim_about() {
        // CP/M states records, not bytes: three records is 384 bytes of
        // allocation and says nothing exact about the file's tail.
        let rounded = SizeClaim {
            bytes: 384,
            basis: SizeBasis::RecordCount {
                record_bytes: 128,
                records: 3,
            },
        };
        assert_eq!(SizeClaim::exact(384).bytes, rounded.bytes);
        assert_ne!(SizeClaim::exact(384).basis, rounded.basis);
    }

    #[test]
    fn a_flat_namespace_is_one_root_of_leaves() {
        let mut provider = SyntheticProvider::new(8);
        let root = provider.add(ItemKind::Directory, Vec::new());
        provider.roots.push(root);
        let file = provider.add_file(SizeClaim::exact(4), Vec::new());
        provider.link(root, RecordedName::utf8("A"), file);

        assert_eq!(provider.roots().expect("roots answer").len(), 1);
        let error = provider
            .entries(file)
            .expect_err("a file is not a directory");
        assert!(error.to_string().contains("holds no names"), "{error}");
        check_conformance(&provider).expect("the contract holds");
    }

    #[test]
    fn a_refusal_is_attributed_to_the_provider_that_presents() {
        let provider = SyntheticProvider::new(4);
        let error = provider
            .item(ItemRef(3))
            .expect_err("the view holds no item 3");
        assert_eq!(
            error.to_string(),
            format!("invalid {PROVIDER} disk image: this view holds no item 3")
        );
    }
}
