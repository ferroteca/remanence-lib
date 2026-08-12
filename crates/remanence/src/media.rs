// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The media pool and the medium a caller holds.
//!
//! **The medium is the content handle.** A [`Session`](crate::Session)
//! owns two pools — machines, which are configuration, and media, which
//! are state — and everything the recording can answer answers here:
//! what the artifact is, what claim it holds, what the open established,
//! its layers, its spaces and its files, and the commit point beneath
//! them all. A device is the slot it may be linked to, not the thing it
//! is.
//!
//! **A medium is created by a declared reading.**
//! [`Session::load_media`](crate::Session::load_media) takes the
//! caller's own opened [`std::fs::File`] and one concrete [`Format`],
//! checked by that format's own adapter and refused by name where the
//! evidence cannot bear it. A classification could check nothing and is
//! not a declaration; a caller who does not yet know what an artifact is
//! asks [`discover_media`](crate::discover_media), which answers the
//! other question.
//!
//! **The pool outlives every machine.** A medium is loaded unlinked, may
//! be inserted into a device and ejected again — ejecting severs and
//! destroys nothing, so the claim and everything buffered survive — and
//! is destroyed only by
//! [`Session::release_media`](crate::Session::release_media) or by the
//! session going away. That is what lets a disk mastered out of an
//! archive outlive the archive it came from.

use std::fmt;
use std::path::Path;

use crate::archive::ArchiveMedium;
use crate::assurance::Assurance;
use crate::device::AccessMode;
use crate::disk::{DiskFormat, MediumState};
use crate::discovery::Discovery;
use crate::error::{Error, Result};
use crate::fat::FatEntry;
use crate::filesystem::{Catalog, StorageSpace};
use crate::filesystem_catalog::{CatalogRecognition, FilesystemAdapter};
use crate::media_profile::MediaProfile;
use crate::report::{DiskReport, VolumeId};
use crate::session::Identification;
use crate::storage_device::AttachmentId;

/// How a refusal names an artifact whose handle may have no name.
pub(crate) fn named(path: Option<&str>) -> String {
    match path {
        Some(path) => format!("'{path}'"),
        None => "this medium (its source handle has no recoverable name)".to_owned(),
    }
}

/// One concrete artifact format, declared at the load.
///
/// **A declaration names a concrete catalog entry, never a
/// classification** (P3): "an archive" or "some floppy image" could
/// check nothing, so the set below is exactly the formats this release
/// reads, each checked by its own adapter. A format this release does
/// not claim fails to compile rather than being spelled and refused at
/// run time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Format {
    /// Bytes, and nothing else — the geometry-opaque reading. It records
    /// no ecosystem and declares no structure, so nothing about the
    /// artifact can contradict it; an artifact belonging to a family no
    /// device holds is still refused, because a raw reading of it would
    /// declare the block layer authoritative where its own adapter
    /// declares otherwise (P13).
    Raw,
    /// QEMU copy-on-write, versions 2 and 3, backing chains composed.
    Qcow2,
    /// VirtualBox disk image, differencing chains composed.
    Vdi,
    /// Heathkit H8/H17 disk image.
    H8d,
    /// ZIP, whose content is a namespace.
    Zip,
    /// 7z, whose content is a namespace.
    SevenZip,
}

impl Format {
    /// Every format a load may declare.
    pub const ALL: [Self; 6] = [
        Self::Raw,
        Self::Qcow2,
        Self::Vdi,
        Self::H8d,
        Self::Zip,
        Self::SevenZip,
    ];

    /// The stable cross-language spelling, which is what the C and
    /// Python surfaces carry and what a refusal quotes back.
    pub const fn id(self) -> &'static str {
        match self {
            Self::Raw => "raw",
            Self::Qcow2 => "qcow2",
            Self::Vdi => "vdi",
            Self::H8d => "h8d",
            Self::Zip => "zip",
            Self::SevenZip => "7z",
        }
    }

    /// The format's name, fit to show a user.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Raw => "Raw disk image",
            Self::Qcow2 => "QEMU copy-on-write disk image",
            Self::Vdi => "VirtualBox disk image",
            Self::H8d => "Heathkit H8 H17 disk image",
            Self::Zip => "ZIP archive",
            Self::SevenZip => "7z archive",
        }
    }

    /// Reads a format back from its stable spelling, for the C and
    /// Python surfaces where a declaration arrives as text. A spelling
    /// this release does not claim is refused naming what is claimed
    /// (P3).
    pub fn from_id(id: &str) -> Result<Self> {
        Self::ALL.into_iter().find(|format| format.id() == id).ok_or_else(|| {
            let claimed: Vec<&str> = Self::ALL.iter().map(|format| format.id()).collect();
            Error::unsupported(format!(
                "'{id}' names no format this release loads; the declarations \
                 it claims are {}",
                claimed.join(", ")
            ))
        })
    }

    /// The archive grammar this format is, where it is one.
    ///
    /// An archive's native vantage is a namespace rather than a space
    /// (P14 as amended), so the two kinds of medium are built by
    /// different adapters and this is the fork between them.
    pub(crate) fn archive_grammar(self) -> Option<&'static str> {
        match self {
            Self::Zip | Self::SevenZip => Some(self.id()),
            Self::Raw | Self::Qcow2 | Self::Vdi | Self::H8d => None,
        }
    }
}

impl fmt::Display for Format {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.id())
    }
}

/// A medium's identity within its session's pool.
///
/// It is issued by the pool, opaque, and never reused for the session's
/// life — a released medium's identity does not come back on the next
/// load, so an identity kept past a release resolves to absence rather
/// than to a stranger.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MediaId(u64);

impl MediaId {
    pub(crate) const fn new(value: u64) -> Self {
        Self(value)
    }

    /// The identity a value names, for the C and Python surfaces, where
    /// one arrives as a number. It is not a lookup: an identity the pool
    /// never issued simply resolves to absence.
    pub const fn from_value(value: u64) -> Self {
        Self(value)
    }

    /// The identity's value, for the C and Python surfaces.
    pub const fn value(self) -> u64 {
        self.0
    }
}

impl fmt::Display for MediaId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "media{}", self.0)
    }
}

/// Where a medium is currently linked: the machine whose device holds
/// it, and that device's slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MediaLink {
    /// Null for the session's anonymous machine.
    pub(crate) machine: Option<String>,
    pub(crate) attachment: AttachmentId,
}

impl fmt::Display for MediaLink {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.machine {
            Some(machine) => write!(f, "{} of machine '{machine}'", self.attachment),
            None => write!(f, "{} of the anonymous machine", self.attachment),
        }
    }
}

/// One loaded medium: the content handle, pool-owned and holdable.
///
/// Every content verb lives here. A medium answers whether or not a
/// device links it — a disk mastered out of an archive answers before
/// any machine exists to seat it — and the verbs a namespace-native
/// medium has no space for refuse by name rather than by failing further
/// in.
#[derive(Debug)]
pub struct Medium {
    id: MediaId,
    state: MediumState,
    link: Option<MediaLink>,
}

impl Medium {
    pub(crate) fn new(id: MediaId, state: MediumState) -> Self {
        Self {
            id,
            state,
            link: None,
        }
    }

    /// This medium's identity in its session's pool — what a device is
    /// told to insert, and what a lookup answers by.
    pub fn id(&self) -> MediaId {
        self.id
    }

    /// The device this medium is currently linked to, where one links it.
    pub(crate) fn link(&self) -> Option<&MediaLink> {
        self.link.as_ref()
    }

    pub(crate) fn set_link(&mut self, link: Option<MediaLink>) {
        self.link = link;
    }

    /// Whether a device currently links this medium. An unlinked medium
    /// is ordinary rather than idle: it is loaded, claimed, and answering.
    pub fn is_linked(&self) -> bool {
        self.link.is_some()
    }

    /// The artifact claimed — the archive itself for an image loaded out
    /// of one — or `None` where the caller's handle has no recoverable
    /// name.
    pub fn path(&self) -> Option<&str> {
        self.state.path()
    }

    /// The resolved artifact — the entry name for an image loaded out of
    /// an archive, else the source's own name.
    pub fn image_path(&self) -> Option<&Path> {
        self.state.image_path()
    }

    /// The resolved image's own size in bytes — the raw plane.
    ///
    /// Distinct from [`Medium::size`], which is the size of the disk the
    /// format adapter presents. For a raw image they agree; for a qcow2
    /// they do not.
    pub fn image_size_bytes(&self) -> u64 {
        self.state.image_size_bytes()
    }

    /// Reads `buf` from the resolved image at `offset` — the medium's own
    /// bytes, not the presented disk. This is the bounded access form
    /// (P27): the image streams from its backing through the session
    /// cache, and no operation requires it resident whole.
    pub fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<()> {
        self.state.read_at(offset, buf)
    }

    /// Identifies the artifact's nesting layers and probable filesystem.
    /// Probes read bounded evidence — a leading prefix, the length, and
    /// the name — never the whole image (P27).
    pub fn identify(&self) -> Identification {
        self.state.identify()
    }

    /// The session's **effective** access mode for this medium: what the
    /// claim afforded where the evidence supports it, and read-only where
    /// it does not.
    ///
    /// A medium loaded on a writable handle whose evidence came up short
    /// reports read-only here and says why in [`Medium::assurance`]
    /// (P28).
    pub fn mode(&self) -> AccessMode {
        self.state.mode()
    }

    /// What the open established about the evidence beneath this medium
    /// (P28): the outcome, the condition where one narrowed the session,
    /// the ordered evidence, the exact extents that read, the access the
    /// evidence permits, and whose open the claim is.
    ///
    /// It is available immediately, before anything is read, so a caller
    /// meets a deficiency by being told rather than by an operation
    /// failing halfway.
    pub fn assurance(&self) -> &Assurance {
        self.state.assurance()
    }

    /// The image container format the medium's image turned out to be,
    /// or the refusal naming a medium that is no disk image.
    pub fn format(&self) -> Result<DiskFormat> {
        Ok(self.state.space("format")?.format())
    }

    /// The presented disk's size (the guest-visible size for qcow2), or
    /// the refusal naming a medium that presents no disk.
    pub fn size(&self) -> Result<u64> {
        Ok(self.state.space("size")?.size())
    }

    /// Whether uncommitted changes exist.
    pub fn is_modified(&self) -> bool {
        self.state.is_modified()
    }

    /// The media type this medium is (P14), by the catalog's stable
    /// spelling — the fact a device's family is checked against when the
    /// medium is inserted.
    pub fn media_type(&self) -> &'static str {
        self.state.media().id
    }

    /// The medium's name, fit to show a user beside the drive it goes in.
    pub fn media_type_name(&self) -> &'static str {
        self.state.media().name
    }

    /// The media profile itself, for the insert check.
    pub(crate) fn media(&self) -> &'static MediaProfile {
        self.state.media()
    }

    /// The layered inspection of this medium: the block-active device,
    /// what its leading structure turned out to be, any recognized
    /// partition schema, every region that schema declares, every volume
    /// actually composed, and every filesystem recognition attempted on
    /// one.
    ///
    /// Each fact stays at the seam that owns it. A region whose type this
    /// release declines to read is still reported, with a reading of what
    /// the type declares and the refusal beside it; a volume whose
    /// filesystem could not be recognized is still a volume, with the
    /// refusal at the filesystem seam; and neither renumbers what follows.
    ///
    /// Content no adapter claims is an outcome here rather than a
    /// refusal — a disk in no format this release knows is a fact about
    /// the disk. An image that cannot be *read* still fails.
    pub fn inspect(&mut self) -> Result<DiskReport> {
        self.state.space_mut("inspect")?.inspect()
    }

    /// The filesystem this medium resolves to, or the refusal that says
    /// why it does not resolve to exactly one (P19, P10).
    ///
    /// **The medium carries no file access of its own.** This is a query
    /// about what it resolves to, whose answer set already includes
    /// *refuse* and *absent*; the file verbs live on the
    /// [`StorageSpace`] it answers with and nowhere else. Where several
    /// volumes bear one, select with [`Medium::volume`] rather than
    /// being guessed for.
    pub fn filesystem(&mut self) -> Result<StorageSpace<'_>> {
        StorageSpace::resolve(self)
    }

    /// One space of this medium, by the identity the inspection report
    /// issued for its volume — the selector where several namespaces
    /// exist, and the way to reach a volume bearing none.
    ///
    /// It answers with the same [`StorageSpace`] the resolver does:
    /// addressable because a volume composed it, and bearing a namespace
    /// only where one was recognized on it.
    pub fn volume(&mut self, id: VolumeId) -> Result<StorageSpace<'_>> {
        self.state.space("volume")?;
        StorageSpace::select(self, id)
    }

    /// The commit point (P2): writes everything buffered since the medium
    /// was loaded (or the last commit/rollback) through to the image,
    /// then flushes. The commit is durable (P9): every host write is
    /// staged in memory first, the bytes it will overwrite are made
    /// durable in a private recovery journal, and only then does the file
    /// change — so an interruption at any point leaves state the next
    /// open reconciles to wholly the old image or wholly the committed
    /// new one. A write-through refusal (P6) likewise surfaces before a
    /// single byte of the file has moved.
    ///
    /// The journal lands **beside the artifact**, so a medium whose
    /// source handle has no recoverable name refuses here by name rather
    /// than committing without it.
    pub fn commit(&mut self) -> Result<()> {
        self.state.space_mut("commit")?.commit()
    }

    /// Discards everything buffered; the image is untouched. Unaltered
    /// cached extents stay resident — they still mirror the image.
    pub fn rollback(&mut self) -> Result<()> {
        self.state.space_mut("rollback")?.rollback();
        Ok(())
    }

    // ------------------------------------- the plumbing the spaces read

    /// Reads within a space's extent, the offset already resolved against
    /// the presented disk by the space that owns the bound.
    pub(crate) fn read_space_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<()> {
        self.state.space_mut("read_at")?.read_space_at(offset, buf)
    }

    /// Writes within a space's extent, buffered until commit like every
    /// other write (P2).
    pub(crate) fn write_space_at(&mut self, offset: u64, data: &[u8]) -> Result<()> {
        self.state.space_mut("write_at")?.write_space_at(offset, data)
    }

    /// The namespace an archive medium bears, where this is one.
    ///
    /// An archive's content *is* its namespace, so nothing is probed for
    /// and nothing is refused: the grammar that recognized the artifact
    /// already read the index this presents.
    pub(crate) fn archive_namespace(&mut self) -> Option<(&'static str, Box<dyn Catalog>)> {
        self.state.archive().map(ArchiveMedium::namespace)
    }

    /// A discovery over one entry of the archive this medium is — the
    /// nested journey, reached from the file view that names the entry.
    ///
    /// The entry is read through the archive's own claim, so the child is
    /// opened without re-opening anything and the artifact cannot change
    /// between the naming and the load (P7).
    pub(crate) fn discover_entry(&mut self, name: &str) -> Result<Discovery> {
        let archive = self.state.archive().ok_or_else(|| {
            Error::unsupported(format!(
                "{} is no archive medium, and this release mints a discovery \
                 from an archive entry alone",
                self.state.named()
            ))
        })?;
        let cache_bytes = archive.cache_bytes();
        let resolved = archive.resolve_entry(name)?;
        Ok(Discovery::over(MediumState::open_entry(
            resolved,
            cache_bytes,
        )?))
    }

    /// Which enrolled adapter claims the namespace this medium bears
    /// directly, for the resolver above.
    pub(crate) fn recognize_namespace(&mut self) -> Result<CatalogRecognition> {
        self.state.space_mut("filesystem")?.recognize_namespace()
    }

    /// Opens the namespace `adapter` recognized — the adapter that
    /// recognized it is the one that reads it.
    pub(crate) fn open_namespace(
        &mut self,
        adapter: &'static dyn FilesystemAdapter,
    ) -> Result<Box<dyn Catalog>> {
        self.state.space_mut("filesystem")?.open_namespace(adapter)
    }

    pub(crate) fn entries(&mut self, volume_id: VolumeId, path: &str) -> Result<Vec<FatEntry>> {
        self.state.space_mut("entries")?.entries(volume_id, path)
    }

    pub(crate) fn stat(&mut self, volume_id: VolumeId, path: &str) -> Result<Option<FatEntry>> {
        self.state.space_mut("stat")?.stat(volume_id, path)
    }

    pub(crate) fn read_file(&mut self, volume_id: VolumeId, path: &str) -> Result<Vec<u8>> {
        self.state.space_mut("read_file")?.read_file(volume_id, path)
    }

    pub(crate) fn read_file_at(
        &mut self,
        volume_id: VolumeId,
        path: &str,
        offset: u64,
        buf: &mut [u8],
    ) -> Result<()> {
        self.state
            .space_mut("read_file_at")?
            .read_file_at(volume_id, path, offset, buf)
    }

    pub(crate) fn resize_file(
        &mut self,
        volume_id: VolumeId,
        path: &str,
        size: u64,
    ) -> Result<()> {
        self.state
            .space_mut("resize_file")?
            .resize_file(volume_id, path, size)
    }

    pub(crate) fn write_file_at(
        &mut self,
        volume_id: VolumeId,
        path: &str,
        offset: u64,
        data: &[u8],
    ) -> Result<()> {
        self.state
            .space_mut("write_file_at")?
            .write_file_at(volume_id, path, offset, data)
    }

    pub(crate) fn write_file(
        &mut self,
        volume_id: VolumeId,
        path: &str,
        contents: &[u8],
    ) -> Result<()> {
        self.state
            .space_mut("write_file")?
            .write_file(volume_id, path, contents)
    }

    pub(crate) fn make_directory(&mut self, volume_id: VolumeId, path: &str) -> Result<()> {
        self.state
            .space_mut("make_directory")?
            .make_directory(volume_id, path)
    }
}

/// The session's media pool: every medium it holds, and the identities
/// they answer to.
///
/// It is not a cache and not a registry of artifacts — it is where media
/// *live*. Nothing here indexes by path, because a medium may have no
/// name and two media may have the same one.
#[derive(Debug, Default)]
pub(crate) struct MediaPool {
    media: Vec<Medium>,
    /// The next identity to issue. It only ever counts up, so a released
    /// medium's identity is never handed to another.
    next: u64,
}

impl MediaPool {
    /// Takes an opened medium into the pool, unlinked, and answers with
    /// the identity it was issued.
    pub(crate) fn admit(&mut self, state: MediumState) -> MediaId {
        let id = MediaId::new(self.next);
        self.next += 1;
        self.media.push(Medium::new(id, state));
        id
    }

    /// Every medium the pool holds, in the order they were loaded.
    pub(crate) fn ids(&self) -> Vec<MediaId> {
        self.media.iter().map(Medium::id).collect()
    }

    pub(crate) fn get(&self, id: MediaId) -> Option<&Medium> {
        self.media.iter().find(|medium| medium.id == id)
    }

    pub(crate) fn get_mut(&mut self, id: MediaId) -> Option<&mut Medium> {
        self.media.iter_mut().find(|medium| medium.id == id)
    }

    /// The medium `id` names, or the refusal naming the absence.
    pub(crate) fn require(&mut self, id: MediaId) -> Result<&mut Medium> {
        self.get_mut(id)
            .ok_or_else(|| Error::not_found(format!("this session holds no {id}")))
    }

    /// Takes a medium out of the pool — the one state-destroying act.
    pub(crate) fn take(&mut self, id: MediaId) -> Result<Medium> {
        let at = self
            .media
            .iter()
            .position(|medium| medium.id == id)
            .ok_or_else(|| Error::not_found(format!("this session holds no {id}")))?;
        Ok(self.media.remove(at))
    }

    /// Severs every link naming `machine` — the cascade a machine's
    /// teardown runs, which takes no state with it.
    pub(crate) fn sever_machine(&mut self, machine: Option<&str>) {
        for medium in &mut self.media {
            if medium.link().is_some_and(|link| link.machine.as_deref() == machine) {
                medium.set_link(None);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_declared_format_round_trips_through_its_spelling() {
        for format in Format::ALL {
            assert_eq!(Format::from_id(format.id()).expect("claimed"), format);
            assert!(!format.name().is_empty());
        }
    }

    #[test]
    fn a_classification_is_not_a_declaration() {
        // P3: the set is enumerated, so a word that names a kind rather
        // than one catalog entry is refused naming what is claimed.
        let error = Format::from_id("archive").expect_err("refused");
        let message = error.to_string();
        assert!(message.contains("archive"), "names what was asked: {message}");
        assert!(message.contains("zip"), "names what is claimed: {message}");
    }

    #[test]
    fn only_the_archive_grammars_declare_one() {
        assert_eq!(Format::Zip.archive_grammar(), Some("zip"));
        assert_eq!(Format::SevenZip.archive_grammar(), Some("7z"));
        for format in [Format::Raw, Format::Qcow2, Format::Vdi, Format::H8d] {
            assert_eq!(format.archive_grammar(), None);
        }
    }

    #[test]
    fn an_identity_is_never_reused_within_a_session() {
        // A released medium's identity resolving to a stranger would make
        // every handle a caller kept a liability.
        let mut pool = MediaPool::default();
        assert_eq!(pool.ids(), Vec::new());
        assert_eq!(MediaId::new(3).to_string(), "media3");
        assert_eq!(MediaId::new(3).value(), 3);
        assert!(pool.get(MediaId::new(0)).is_none());
        assert!(pool.take(MediaId::new(0)).is_err());
    }
}
