// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The storage-device tier (P32): a machine holds a set of family-typed
//! devices, and a device is a durable slot distinct from whatever medium
//! currently occupies it.
//!
//! **The device is the slot, not the disk, and here that is the whole of
//! what it is.** A device carries its attachment identity, its family,
//! and at most one link to a medium in the session's pool. Every content
//! verb lives on [`Medium`](crate::Medium): the recording answers for
//! itself whether or not a drive is configured for it, which is what
//! lets a disk mastered out of an archive answer before any machine
//! exists to seat it, and what lets a machine be torn down without
//! taking a disk with it.
//!
//! **Devices are added; media are loaded; the two are linked — three
//! acts.** A machine adds the device
//! ([`MachineView::add_device`](crate::MachineView::add_device)), the
//! session loads the medium
//! ([`Session::load_media`](crate::Session::load_media)), and
//! [`DeviceView::insert`] links them. That is what makes an *empty*
//! device first-class configuration: the drive U22 letters whether or not
//! a disk is in it, and "insert the disk" cannot hang off the disk.
//!
//! **The edge crosses configuration into state, and only in one
//! direction.** Insert checks the device's family against the medium
//! (P14) and refuses naming both sides. [`DeviceView::eject`] **severs
//! only** — the medium stays in the pool with its claim, its assurance
//! and everything buffered intact — so nothing about ejecting is a
//! commit point and nothing about it is destructive. The one
//! state-destroying verb in the model is
//! [`Session::release_media`](crate::Session::release_media).

use std::fmt;

use crate::device_family::DeviceFamily;
use crate::error::{Error, Result};
use crate::media::{MediaId, MediaLink, MediaPool, Medium};

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

/// One storage device: a durable, family-typed slot, and the link to
/// whichever pooled medium currently occupies it.
///
/// The medium is the disk currently inserted; the device outlives it,
/// and so — pool-held — does the disk.
#[derive(Debug)]
pub struct StorageDevice {
    attachment: AttachmentId,
    medium: Option<MediaId>,
}

impl StorageDevice {
    /// A device in its slot, empty. **An empty device is first-class
    /// configuration** — the drive U22 letters whether or not a disk is
    /// in it — so this is the only way one is made, and linking a medium
    /// is a separate act.
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

    /// The identity of the medium in the slot, where one is linked.
    pub fn media_id(&self) -> Option<MediaId> {
        self.medium
    }

    pub(crate) fn set_media_id(&mut self, id: Option<MediaId>) {
        self.medium = id;
    }
}

/// A device reached with its session's media pool beside it — the handle
/// the edge verbs live on.
///
/// A [`StorageDevice`] is configuration a machine owns; the pool is state
/// the session owns. Linking them is the one act that crosses, so it is
/// spelled on the object that holds both, and the borrow cannot outlive
/// either.
#[derive(Debug)]
pub struct DeviceView<'a> {
    pub(crate) device: &'a mut StorageDevice,
    pub(crate) pool: &'a mut MediaPool,
    /// Null for the session's anonymous machine.
    pub(crate) machine: Option<String>,
}

impl DeviceView<'_> {
    /// This device's attachment identity.
    pub fn attachment(&self) -> AttachmentId {
        self.device.attachment()
    }

    pub fn family(&self) -> DeviceFamily {
        self.device.family()
    }

    /// Whether a medium currently occupies the slot.
    pub fn is_occupied(&self) -> bool {
        self.device.is_occupied()
    }

    /// The identity of the medium in the slot, where one is linked.
    pub fn media_id(&self) -> Option<MediaId> {
        self.device.media_id()
    }

    /// The device this view names, as configuration.
    pub fn device(&self) -> &StorageDevice {
        self.device
    }

    /// Links the pooled medium `media` into this slot.
    ///
    /// **A device accepts only the media its family is served** (P14),
    /// and a medium belonging in another drive is refused naming both
    /// sides rather than read as something it is not — which is the check
    /// a concrete family exists to make possible.
    ///
    /// Three other refusals guard the edge, each naming what is already
    /// true: an identity the pool does not hold, a slot already occupied
    /// (ejecting is [`DeviceView::eject`] and it is a separate act), and
    /// a medium some other slot already holds — one disk is in one drive,
    /// and two drives sharing one would be a machine no machine was.
    pub fn insert(&mut self, media: MediaId) -> Result<()> {
        let attachment = self.device.attachment();
        if let Some(occupant) = self.device.media_id() {
            return Err(Error::unsupported(format!(
                "{attachment} already holds {occupant}; eject it before \
                 inserting another medium"
            )));
        }
        let family = self.device.family();
        // A link is not a lookup: the pool's absence is refused by name
        // here, because the caller named an identity to link rather than
        // asking what the pool holds.
        let medium = self
            .pool
            .get_mut(media)
            .ok_or_else(|| Error::not_found(format!("this session holds no {media}")))?;
        if let Some(link) = medium.link() {
            return Err(Error::unsupported(format!(
                "{media} is already in {link}; eject it there before inserting \
                 it here"
            )));
        }
        if !family.accepts(medium.media()) {
            return Err(Error::unsupported(format!(
                "{media} holds {} and {attachment} is a {}, which is {}",
                medium.media().name,
                family.name(),
                family.served_reading()
            )));
        }
        medium.set_link(Some(MediaLink {
            machine: self.machine.clone(),
            attachment,
        }));
        self.device.set_media_id(Some(media));
        Ok(())
    }

    /// Severs the link, leaving the device in place and the medium in the
    /// pool — **the device is the slot, not the disk, and the disk is the
    /// session's, not the drive's**.
    ///
    /// Nothing is destroyed and nothing is committed. The claim, the
    /// assurance, and every buffered change survive, so a medium may be
    /// ejected from one drive and inserted into another with no state
    /// crossing the gap. The commit point is explicit (P2), and this is
    /// not it.
    pub fn eject(&mut self) -> Result<MediaId> {
        let attachment = self.device.attachment();
        let media = self.device.media_id().ok_or_else(|| {
            Error::not_found(format!(
                "no medium is in {attachment}; there is nothing to eject"
            ))
        })?;
        if let Some(medium) = self.pool.get_mut(media) {
            medium.set_link(None);
        }
        self.device.set_media_id(None);
        Ok(media)
    }

    /// The medium in this slot, or `None` where the device is empty —
    /// absence being an answer rather than a manufactured error.
    pub fn medium(&self) -> Option<&Medium> {
        self.pool.get(self.device.media_id()?)
    }

    /// The medium in this slot, ready to be worked, or `None` where the
    /// device is empty.
    ///
    /// **A caller who wants a demand writes it**, because only they know
    /// what an empty slot means where they stand: the content verbs live
    /// on the medium, and "there is no disk in the drive" is an ordinary
    /// answer at one call site and a refusal at the next.
    pub fn medium_mut(&mut self) -> Option<&mut Medium> {
        let media = self.device.media_id()?;
        self.pool.get_mut(media)
    }

    /// The medium in this slot, or the refusal naming the empty one —
    /// the demand beside the lookup, where a caller means to work the
    /// content and an empty slot is an error rather than an answer.
    pub fn require_medium(&mut self) -> Result<&mut Medium> {
        let attachment = self.device.attachment();
        let media = self.device.media_id().ok_or_else(|| {
            Error::not_found(format!(
                "no medium is in {attachment}; the content verbs answer on \
                 the medium, and there is none here"
            ))
        })?;
        self.pool.require(media)
    }
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

    #[test]
    fn a_fresh_device_is_an_empty_slot_and_that_is_configuration() {
        let device = StorageDevice::new(AttachmentId::new(DeviceFamily::HARD_DISK, 1));
        assert_eq!(device.attachment().to_string(), "hdd1");
        assert_eq!(device.family(), DeviceFamily::HARD_DISK);
        assert!(!device.is_occupied());
        assert_eq!(device.media_id(), None);
    }
}
