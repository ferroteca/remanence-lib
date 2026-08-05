// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The storage-device tier (P32): a machine holds a set of family-typed
//! devices, and a device is a durable slot distinct from whatever medium
//! currently occupies it.
//!
//! The device is the slot, not the disk. Ejecting a medium and loading
//! another leaves the device where it was, which is what makes this a tier
//! rather than a rename of the medium surface F43 delivered.
//!
//! **Devices are added; media are loaded — as two acts.** A machine adds
//! the device and the device loads the medium, which is what makes an
//! *empty* device first-class configuration: the drive U22 letters
//! whether or not a disk is in it, and "insert the disk" cannot hang off
//! the disk. A device accepts only the media its family is served (P14),
//! and a mismatch is refused naming both sides.
//!
//! **It is also the one storage handle.** A caller never holds a medium
//! outside a device, so the two model nodes are exposed as one object:
//! slot-side facts — the attachment identity, the family, occupancy — and
//! content-side facts — the media type's planes, identification,
//! inspection, the file verbs, commit and rollback — answer on the same
//! handle, with the content verbs refusing by name when the slot is
//! empty. The nodes stay attributed even so: the medium's state is
//! [`crate::disk::MediaState`], private, homed here rather than held.

use std::fmt;
use std::path::Path;

use crate::assurance::Assurance;
use crate::device::{AccessIntent, AccessMode};
use crate::device_family::DeviceFamily;
use crate::disk::{DiskFormat, MediaState};
use crate::error::{Error, Result};
use crate::fat::FatEntry;
use crate::hdos::HdosFile;
use crate::report::{DiskReport, VolumeId};
use crate::session::Identification;

/// A device's attachment identity: its family and the slot it occupies.
///
/// In-force P21 distinguishes "an attachment identity such as `hdd0`" from
/// the opaque device identity the library assigns an addressed virtual
/// device, and this is the former. It is deliberately caller-facing and
/// predictable, unlike the region, volume and filesystem identities an
/// inspection report issues — because a device is machine configuration
/// the caller supplied, not evidence read off a disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AttachmentId {
    family: DeviceFamily,
    index: u32,
}

impl AttachmentId {
    /// The identity of slot `index` in `family`, which must be concrete —
    /// an interior name owns no slot, and every caller of this has
    /// already refused one.
    pub(crate) fn new(family: DeviceFamily, index: u32) -> Self {
        debug_assert!(
            family.is_concrete(),
            "an interior family name owns no slot to identify"
        );
        Self { family, index }
    }

    pub fn family(&self) -> DeviceFamily {
        self.family
    }

    /// The slot within the family. Callers may choose this; they may not
    /// choose a name.
    pub fn index(&self) -> u32 {
        self.index
    }

    /// Parses an identity such as `hdd0`, refusing an unclaimed family or
    /// a malformed slot by name. For the C and Python surfaces, where an
    /// identity arrives as text.
    ///
    /// The family half is a **slot prefix**, not a family's stable
    /// spelling: `cbmfloppy0` is the Commodore 1541's slot, as
    /// `commodore-1541` is its name. The two namespaces are separate
    /// deliberately — a slot reads like device enumeration, and a family
    /// reads like the machine fact it asserts.
    pub fn parse(text: &str) -> Result<Self> {
        let split = text
            .find(|c: char| c.is_ascii_digit())
            .ok_or_else(|| Error::unsupported(format!(
                "'{text}' is not an attachment identity; one reads like 'hdd0'"
            )))?;
        let (prefix, index) = text.split_at(split);
        let family = DeviceFamily::by_slot_prefix(prefix)?;
        let index = index.parse::<u32>().map_err(|_| {
            Error::unsupported(format!(
                "'{text}' is not an attachment identity; '{index}' is not a slot"
            ))
        })?;
        Ok(Self::new(family, index))
    }
}

impl fmt::Display for AttachmentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}{}",
            self.family
                .slot_prefix()
                .expect("a device's family is concrete and names a slot"),
            self.index
        )
    }
}

/// One storage device: a durable, family-typed slot holding zero or one
/// attached medium, and the handle everything about that medium is
/// reached through.
///
/// The medium is the disk currently inserted; the device outlives it.
#[derive(Debug)]
pub struct StorageDevice {
    attachment: AttachmentId,
    medium: Option<MediaState>,
}

impl StorageDevice {
    /// A device in its slot, empty. **An empty device is first-class
    /// configuration** — the drive U22 letters whether or not a disk is
    /// in it — so this is the only way one is made, and loading is the
    /// second act.
    pub(crate) fn new(attachment: AttachmentId) -> Self {
        Self {
            attachment,
            medium: None,
        }
    }

    /// This device's attachment identity — `hdd0` and the like.
    pub fn attachment(&self) -> AttachmentId {
        self.attachment
    }

    pub fn family(&self) -> DeviceFamily {
        self.attachment.family()
    }

    /// Whether a medium currently occupies the slot.
    pub fn is_occupied(&self) -> bool {
        self.medium.is_some()
    }

    /// Loads the medium at `path` — a disk image, or `archive[/entry]` —
    /// into this device under the caller's declared intent (P7), and
    /// hands back nothing to hold: the device is the one storage handle,
    /// and the medium's facts answer on it.
    ///
    /// **A device accepts only the media its family is served** (P14).
    /// The image-format adapter that loads the state names the medium,
    /// and a medium belonging in another drive is refused naming both
    /// sides rather than read as something it is not — which is the check
    /// a concrete family exists to make possible.
    ///
    /// An occupied slot is refused rather than displaced: ejecting is
    /// [`StorageDevice::eject`] and it is a separate act.
    pub fn load_media(&mut self, path: impl AsRef<Path>, intent: AccessIntent) -> Result<()> {
        self.load_media_with_cache(path, intent, crate::DEFAULT_CACHE_BYTES)
    }

    /// [`StorageDevice::load_media`] under a caller-declared session
    /// cache bound (P27).
    pub fn load_media_with_cache(
        &mut self,
        path: impl AsRef<Path>,
        intent: AccessIntent,
        cache_bytes: u64,
    ) -> Result<()> {
        let attachment = self.attachment;
        if self.medium.is_some() {
            return Err(Error::unsupported(format!(
                "{attachment} already holds a medium; eject it before loading \
                 another"
            )));
        }
        let path = path.as_ref();
        let medium = MediaState::open_with_cache(path, intent, cache_bytes)?;

        // A flux artifact is refused whatever the device. A P64 records
        // timed pulses, and the block catalog opens anything it cannot
        // identify at the raw adapter — so without this the block layer
        // would be declared authoritative where the artifact's own
        // adapter declares flux, which in-force P13 forbids. It is
        // reached through its own type, as the capture-set adapter is.
        if let Some(foreign) = foreign_family(&medium) {
            return Err(Error::unsupported(format!(
                "'{}' is a {foreign}-family artifact and no device in this \
                 release holds a {foreign} medium; a {foreign} container is \
                 read through its own type",
                path.display()
            )));
        }

        let media = medium.media();
        if !self.family().accepts(media) {
            return Err(Error::unsupported(format!(
                "'{}' holds {} and {attachment} is a {}, which is {}",
                path.display(),
                media.name,
                self.family().name(),
                self.family().served_reading()
            )));
        }

        self.medium = Some(medium);
        Ok(())
    }

    /// Removes the medium, releasing its P7 claim, and leaves the device
    /// in place — the device is the slot, not the disk.
    ///
    /// Every view taken through this device stops answering: the state
    /// they were views of has left, and the content verbs refuse by name
    /// until another medium is loaded. Buffered changes are discarded
    /// with it, as they are on `remove_device`; the commit point is
    /// explicit (P2), and ejecting is not it.
    pub fn eject(&mut self) -> Result<()> {
        self.take_medium().map(drop)
    }

    /// The medium, taken out of the slot, or a refusal naming the empty
    /// one.
    pub(crate) fn take_medium(&mut self) -> Result<MediaState> {
        let attachment = self.attachment;
        self.medium.take().ok_or_else(|| {
            Error::not_found(format!(
                "no medium is attached to {attachment}; there is nothing to eject"
            ))
        })
    }

    /// The medium's state for a content verb, or that verb's refusal
    /// naming the empty slot. Every content verb below passes through
    /// here, so an empty device answers by name rather than by a verb
    /// failing further in.
    fn media(&self, verb: &str) -> Result<&MediaState> {
        let attachment = self.attachment;
        self.medium.as_ref().ok_or_else(|| {
            Error::not_found(format!(
                "no medium is attached to {attachment}; '{verb}' is a \
                 content verb and needs one"
            ))
        })
    }

    fn media_mut(&mut self, verb: &str) -> Result<&mut MediaState> {
        let attachment = self.attachment;
        self.medium.as_mut().ok_or_else(|| {
            Error::not_found(format!(
                "no medium is attached to {attachment}; '{verb}' is a \
                 content verb and needs one"
            ))
        })
    }

    /// The artifact the medium was opened from — the archive itself for an
    /// image opened from inside one, rather than the `archive/entry` path
    /// as given.
    pub fn path(&self) -> Result<&str> {
        Ok(self.media("path")?.path())
    }

    /// The resolved image — the entry name for an image opened from
    /// inside an archive, else the source path.
    pub fn image_path(&self) -> Result<&Path> {
        Ok(self.media("image_path")?.image_path())
    }

    /// The resolved image's own size in bytes — the raw plane.
    ///
    /// Distinct from [`StorageDevice::size`], which is the size of the
    /// disk the format adapter presents. For a raw image they agree; for
    /// a qcow2 they do not, and conflating them is exactly the confusion
    /// one handle over both planes could have introduced.
    pub fn image_size_bytes(&self) -> Result<u64> {
        Ok(self.media("image_size_bytes")?.image_size_bytes())
    }

    /// Reads `buf` from the resolved image at `offset` — the medium's own
    /// bytes, not the presented disk. This is the bounded access form
    /// (P27): the image streams from its backing through the session
    /// cache, and no operation requires it resident whole.
    pub fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<()> {
        self.media("read_at")?.read_at(offset, buf)
    }

    /// Identifies the image's container layers and probable filesystem.
    /// Probes read bounded evidence — a leading prefix, the length, and
    /// the name — never the whole image (P27).
    pub fn identify(&self) -> Result<Identification> {
        Ok(self.media("identify")?.identify())
    }

    /// Parses the HDOS directory from the image. HDOS images are bounded
    /// small by their formats, so the whole volume is read through the
    /// cache; an image past the bound is refused by size, never loaded
    /// (P27).
    pub fn list_hdos_files(&self) -> Result<Vec<HdosFile>> {
        self.media("list_hdos_files")?.list_hdos_files()
    }

    /// Copies one HDOS file's bytes out of the image, under the same size
    /// bound as [`StorageDevice::list_hdos_files`].
    pub fn read_hdos_file(&self, name: &str) -> Result<Vec<u8>> {
        self.media("read_hdos_file")?.read_hdos_file(name)
    }

    /// The session's **effective** access mode for this medium: the
    /// declared intent's echo where the evidence supports it, and
    /// read-only where it does not.
    ///
    /// A write-intent attach whose evidence came up short reports
    /// read-only here and says why in [`StorageDevice::assurance`] (P28).
    /// The claim taken at the open is unchanged — this is a restriction
    /// after a safe claim, not P7's no-silent-fallback rule, which still
    /// fails an open that cannot claim what it asked for.
    pub fn mode(&self) -> Result<AccessMode> {
        Ok(self.media("mode")?.mode())
    }

    /// What the open established about the evidence beneath this medium
    /// (P28): the outcome, the condition where one narrowed the session,
    /// the ordered evidence, the exact extents that read, and the access
    /// the evidence permits.
    ///
    /// It is available immediately, before anything is read, so a caller
    /// meets a deficiency by being told rather than by an operation
    /// failing halfway.
    pub fn assurance(&self) -> Result<&Assurance> {
        Ok(self.media("assurance")?.assurance())
    }

    /// The container format the medium's image turned out to be.
    pub fn format(&self) -> Result<DiskFormat> {
        Ok(self.media("format")?.format())
    }

    /// The presented disk's size (the guest-visible size for qcow2).
    pub fn size(&self) -> Result<u64> {
        Ok(self.media("size")?.size())
    }

    /// Whether uncommitted changes exist.
    pub fn is_modified(&self) -> Result<bool> {
        Ok(self.media("is_modified")?.is_modified())
    }

    /// The layered inspection of the attached medium: the block-active
    /// device, what its leading structure turned out to be, any
    /// recognized partition schema, every region that schema declares,
    /// every volume actually composed, and every filesystem recognition
    /// attempted on one.
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
        self.media_mut("inspect")?.inspect()
    }

    /// Lists a directory in the volume identified by `volume_id`
    /// ("" = root; "A/B" descends).
    pub fn entries(&mut self, volume_id: VolumeId, path: &str) -> Result<Vec<FatEntry>> {
        self.media_mut("entries")?.entries(volume_id, path)
    }

    /// Answers one path in the volume identified by `volume_id` with its
    /// entry, or `None` when nothing exists at that path — a missing
    /// leaf, a missing parent, or a parent that is a file alike. Absence
    /// is an answer, distinguished from failure to read the volume (U3).
    pub fn stat(&mut self, volume_id: VolumeId, path: &str) -> Result<Option<FatEntry>> {
        self.media_mut("stat")?.stat(volume_id, path)
    }

    /// Copies a file's bytes out of the volume identified by `volume_id`.
    pub fn read_file(&mut self, volume_id: VolumeId, path: &str) -> Result<Vec<u8>> {
        self.media_mut("read_file")?.read_file(volume_id, path)
    }

    /// Reads part of a file — the streamed form (P27), beside
    /// [`StorageDevice::read_file`]: exactly `buf` bytes at `offset`,
    /// which must lie within the file.
    pub fn read_file_at(
        &mut self,
        volume_id: VolumeId,
        path: &str,
        offset: u64,
        buf: &mut [u8],
    ) -> Result<()> {
        self.media_mut("read_file_at")?
            .read_file_at(volume_id, path, offset, buf)
    }

    /// Sets a file's size, creating it when absent: kept bytes preserved
    /// in place, a grown region reads as zeros. With
    /// [`StorageDevice::write_file_at`] this is the streamed replacement
    /// for [`StorageDevice::write_file`]. Buffered until commit.
    pub fn resize_file(&mut self, volume_id: VolumeId, path: &str, size: u64) -> Result<()> {
        self.media_mut("resize_file")?
            .resize_file(volume_id, path, size)
    }

    /// Writes part of a file in place — the streamed form (P27), beside
    /// [`StorageDevice::write_file`]: the span must lie within the file's
    /// current size (resize first to change it). Buffered until commit.
    pub fn write_file_at(
        &mut self,
        volume_id: VolumeId,
        path: &str,
        offset: u64,
        data: &[u8],
    ) -> Result<()> {
        self.media_mut("write_file_at")?
            .write_file_at(volume_id, path, offset, data)
    }

    /// Writes a file into the volume identified by `volume_id`. An
    /// existing file is overwritten — shorter or longer, its old
    /// clusters released and reclaimed, every FAT copy kept in step —
    /// while an existing directory is refused. Buffered until
    /// [`StorageDevice::commit`].
    pub fn write_file(&mut self, volume_id: VolumeId, path: &str, contents: &[u8]) -> Result<()> {
        self.media_mut("write_file")?
            .write_file(volume_id, path, contents)
    }

    /// Ensures a directory exists in the volume identified by
    /// `volume_id`: missing parents are created, and a path that already
    /// leads to a directory — the root included — succeeds unchanged.
    /// Buffered until commit.
    pub fn make_directory(&mut self, volume_id: VolumeId, path: &str) -> Result<()> {
        self.media_mut("make_directory")?
            .make_directory(volume_id, path)
    }

    /// The commit point (P2): writes everything buffered since the medium
    /// was attached (or the last commit/rollback) through to the image,
    /// then flushes. The commit is durable (P9): every host write is
    /// staged in memory first, the bytes it will overwrite are made
    /// durable in a private recovery journal, and only then does the file
    /// change — so an interruption at any point leaves state the next
    /// open reconciles to wholly the old image or wholly the committed
    /// new one. A write-through refusal (P6) likewise surfaces before a
    /// single byte of the file has moved.
    pub fn commit(&mut self) -> Result<()> {
        self.media_mut("commit")?.commit()
    }

    /// Discards everything buffered; the image is untouched. Unaltered
    /// cached extents stay resident — they still mirror the image.
    pub fn rollback(&mut self) -> Result<()> {
        self.media_mut("rollback")?.rollback();
        Ok(())
    }
}

/// The family an artifact belongs to, when it is one this release
/// recognizes and it is not the block family.
///
/// The library can only refuse what it can recognize. An artifact it
/// cannot place at all still opens at the block catalog's raw fallback,
/// which is the honest limit of this check rather than a hole in it: NIB
/// and NBZ, for instance, have no recognizer until the principle that
/// places them at the flux rung is delivered.
fn foreign_family(medium: &MediaState) -> Option<&'static str> {
    let mut prefix = [0u8; 8];
    if medium.read_at(0, &mut prefix).is_err() {
        return None;
    }
    crate::p64::has_signature(&prefix).then_some("flux")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_attachment_identity_reads_as_slot_prefix_and_slot() {
        let id = AttachmentId::new(DeviceFamily::HARD_DISK, 0);
        assert_eq!(id.to_string(), "hdd0");
        assert_eq!(AttachmentId::parse("hdd0").expect("parses"), id);
        assert_eq!(
            AttachmentId::parse("hdd12").expect("parses").index(),
            12,
            "a slot is not a single digit"
        );

        // Every concrete family's slot round-trips, which is what makes
        // the prefix an identity rather than a label.
        for family in DeviceFamily::concrete() {
            let id = AttachmentId::new(family, 3);
            let parsed = AttachmentId::parse(&id.to_string()).expect("parses");
            assert_eq!(parsed, id, "{} does not round-trip", family.id());
            assert_eq!(parsed.family(), family);
        }
    }

    #[test]
    fn an_unclaimed_slot_is_refused_by_name() {
        // P3: the family set is an enumerated claim. An optical drive is
        // the obvious next entry and naming its slot must refuse rather
        // than pretend.
        let error = AttachmentId::parse("cdrom0").expect_err("refused");
        let message = error.to_string();
        assert!(message.contains("cdrom"), "names what was asked: {message}");
        assert!(message.contains("hdd"), "names what is claimed: {message}");
    }

    #[test]
    fn a_family_name_is_not_a_slot_prefix() {
        // The two namespaces are separate deliberately, so neither
        // resolves the other's spelling.
        assert!(
            AttachmentId::parse("commodore-15410").is_err(),
            "a family's stable spelling is not a slot prefix"
        );
        assert!(
            DeviceFamily::from_id("cbmfloppy").is_err(),
            "a slot prefix is not a family's stable spelling"
        );
        assert_eq!(
            AttachmentId::parse("cbmfloppy0").expect("parses").family(),
            DeviceFamily::COMMODORE_1541
        );
    }

    #[test]
    fn a_malformed_identity_is_refused_rather_than_guessed() {
        assert!(AttachmentId::parse("hdd").is_err(), "no slot at all");
        assert!(AttachmentId::parse("").is_err(), "empty");
        assert!(AttachmentId::parse("0").is_err(), "no family");
    }
}
