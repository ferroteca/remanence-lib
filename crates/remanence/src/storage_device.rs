// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The storage-device tier (P32): a machine holds a set of devices, and
//! a device is a durable slot distinct from whatever medium currently
//! occupies it.
//!
//! **The device is the slot, not the disk, and here that is the whole of
//! what it is.** A device carries its attachment identity, the
//! [`DeviceSlot`] it is typed by — a recording device of one concrete
//! type, or the archive receiver — and at most one link to a medium in
//! the session's pool. Every content verb lives on
//! [`Medium`](crate::Medium): the recording answers for itself whether
//! or not a drive is configured for it, which is what lets a disk
//! mastered out of an archive answer before any machine exists to seat
//! it, and what lets a machine be torn down without taking a disk with
//! it.
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
//! direction.** Insert is **device-type equality**, naming both sides: a
//! medium is recorded by one device type and a slot is typed by one, so
//! a 1541 refuses an H-37 disk it could physically hold but never serve
//! — a check the article alone cannot make, both being the same
//! soft-sectored 5.25-inch disk. [`DeviceView::eject`] **severs only** —
//! the medium stays in the pool with its claim, its assurance and
//! everything buffered intact — so nothing about ejecting is a commit
//! point and nothing about it is destructive. The one state-destroying
//! verb in the model is
//! [`Session::release_media`](crate::Session::release_media).

use std::fmt;

use crate::device_type::{DeviceSlot, DeviceType};
use crate::error::{Error, Result};
use crate::media::{MediaId, MediaLink, MediaPool, Medium};

/// A device's attachment identity: the slot it occupies, spelled as its
/// prefix and index.
///
/// In-force P21 distinguishes "an attachment identity such as `hdd0`" from
/// the opaque device identity the library assigns an addressed virtual
/// device, and this is the former. It is deliberately caller-facing and
/// predictable, unlike the region, volume and filesystem identities an
/// inspection report issues — because a device is machine configuration
/// the caller supplied, not evidence read off a disk.
///
/// **An identity names a place, never a recording.** Several device
/// types fill one kind of bay — three hard-drive types take `hdd`, both
/// Heathkit controllers take `heathfloppy` — so the type a device is
/// lives on the device, and asking a machine for `hdd0` asks for
/// whatever drive is in that bay.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AttachmentId {
    prefix: &'static str,
    index: u32,
}

impl AttachmentId {
    /// The identity of slot `index` in the bay `slot` fills.
    pub(crate) fn new(slot: DeviceSlot, index: u32) -> Self {
        Self {
            prefix: slot.slot_prefix(),
            index,
        }
    }

    /// The family half of the identity — `hdd` for `hdd0`.
    pub fn prefix(&self) -> &'static str {
        self.prefix
    }

    /// The slot within the bay. Callers may choose this; they may not
    /// choose a name.
    pub fn index(&self) -> u32 {
        self.index
    }

    /// Parses an identity such as `hdd0`, refusing an unclaimed bay or
    /// a malformed slot by name. For the C and Python surfaces, where an
    /// identity arrives as text.
    ///
    /// The prefix is a **slot prefix**, not a device type's stable
    /// spelling: `cbmfloppy0` is the slot a Commodore 1541 fills, as
    /// `c1541` is the type's name. The two namespaces are separate
    /// deliberately — a slot reads like device enumeration, and a device
    /// type reads like the recording fact it asserts.
    pub fn parse(text: &str) -> Result<Self> {
        let split = text.find(|c: char| c.is_ascii_digit()).ok_or_else(|| {
            Error::unsupported(format!(
                "'{text}' is not an attachment identity; one reads like 'hdd0'"
            ))
        })?;
        let (prefix, index) = text.split_at(split);
        let prefix = DeviceSlot::claimed_prefix(prefix).ok_or_else(|| {
            Error::unsupported(format!(
                "no device this release claims takes the slot prefix \
                 '{prefix}'; the claimed slots are {}",
                DeviceSlot::prefixes().join(", ")
            ))
        })?;
        let index = index.parse::<u32>().map_err(|_| {
            Error::unsupported(format!(
                "'{text}' is not an attachment identity; '{index}' is not a slot"
            ))
        })?;
        Ok(Self { prefix, index })
    }
}

impl fmt::Display for AttachmentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.prefix, self.index)
    }
}

/// One storage device: a durable slot typed by what fills it, and the
/// link to whichever pooled medium currently occupies it.
///
/// The medium is the disk currently inserted; the device outlives it,
/// and so — pool-held — does the disk.
#[derive(Debug)]
pub struct StorageDevice {
    attachment: AttachmentId,
    slot: DeviceSlot,
    medium: Option<MediaId>,
}

impl StorageDevice {
    /// A device in its slot, empty. **An empty device is first-class
    /// configuration** — the drive U22 letters whether or not a disk is
    /// in it — so this is the only way one is made, and linking a medium
    /// is a separate act.
    pub(crate) fn new(slot: DeviceSlot, attachment: AttachmentId) -> Self {
        Self {
            attachment,
            slot,
            medium: None,
        }
    }

    /// This device's attachment identity — `hdd0` and the like.
    pub fn attachment(&self) -> AttachmentId {
        self.attachment
    }

    /// What this device is: a recording device of one concrete type, or
    /// the archive receiver.
    pub fn slot(&self) -> DeviceSlot {
        self.slot
    }

    /// The recording device type this slot is typed by, or `None` for
    /// the archive receiver — which records nothing, as the archive it
    /// holds was recorded by nothing.
    pub fn device_type(&self) -> Option<DeviceType> {
        self.slot.device_type()
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

    /// What this device is.
    pub fn slot(&self) -> DeviceSlot {
        self.device.slot()
    }

    /// The recording device type this slot is typed by, or `None` for
    /// the archive receiver.
    pub fn device_type(&self) -> Option<DeviceType> {
        self.device.device_type()
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
    /// **The check is device-type equality**, and the refusal names both
    /// sides: a medium carries the device type its content was recorded
    /// by and a slot is typed by the device that fills it, so a 1541
    /// refuses an H-37 disk it could physically hold but never serve.
    /// The article cannot make that check — both are the same
    /// soft-sectored 5.25-inch disk — which is why the recording is a
    /// fact of its own (D19).
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
        let slot = self.device.slot();
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
        // An authored blank goes in no slot at all: nothing recorded it,
        // so there is no recording for the equality check to weigh, and
        // seating it somewhere would assert the drive the author
        // deliberately did not. Only the reserved authored-to-recorded
        // arc binds one.
        let Some(recorded) = medium.slot() else {
            return Err(Error::unsupported(format!(
                "{media} was created whole by its author and assumes no device \
                 — authorship is its own fact class, and device_type() answers \
                 none — so no drive takes it; {attachment} takes {}",
                served_reading(slot),
            )));
        };
        if recorded != slot {
            return Err(Error::unsupported(format!(
                "{media} holds {}, and {attachment} takes {}",
                crate::device_type::recorded_reading(recorded.device_type()),
                served_reading(slot),
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
}

/// What a slot takes, in a phrase fit to follow "takes", for the seam
/// that found a medium in the wrong drive.
fn served_reading(slot: DeviceSlot) -> String {
    match slot {
        DeviceSlot::Recorded(device) => format!(
            "{} ({}) recordings, on {}",
            device.name(),
            device.id(),
            device.article_profile().name
        ),
        DeviceSlot::Archive => "archives, which no device recorded".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device_type::{FloppyDrive, HardDrive};

    #[test]
    fn an_attachment_identity_reads_as_slot_prefix_and_slot() {
        let id = AttachmentId::new(HardDrive::MbrBlock.into(), 0);
        assert_eq!(id.to_string(), "hdd0");
        assert_eq!(AttachmentId::parse("hdd0").expect("parses"), id);
        assert_eq!(
            AttachmentId::parse("hdd12").expect("parses").index(),
            12,
            "a slot is not a single digit"
        );

        // Every claimed slot round-trips, which is what makes the prefix
        // an identity rather than a label.
        for slot in DeviceSlot::claimed() {
            let id = AttachmentId::new(slot, 3);
            let parsed = AttachmentId::parse(&id.to_string()).expect("parses");
            assert_eq!(parsed, id, "{} does not round-trip", slot.id());
            assert_eq!(parsed.prefix(), slot.slot_prefix());
        }
    }

    #[test]
    fn an_identity_names_a_place_and_not_a_recording() {
        // Two device types in one bay answer to one identity, which is
        // why the type lives on the device rather than in the name.
        assert_eq!(
            AttachmentId::new(HardDrive::MbrSector.into(), 0),
            AttachmentId::new(HardDrive::Gpt.into(), 0),
        );
        assert_eq!(
            AttachmentId::new(FloppyDrive::HeathH17.into(), 1).to_string(),
            "heathfloppy1"
        );
        assert_eq!(
            AttachmentId::new(FloppyDrive::HeathH37.into(), 1).to_string(),
            "heathfloppy1"
        );
    }

    #[test]
    fn an_unclaimed_slot_is_refused_by_name() {
        // P3: the device catalog is an enumerated claim. An optical
        // drive is the obvious next entry and naming its slot must
        // refuse rather than pretend.
        let error = AttachmentId::parse("cdrom0").expect_err("refused");
        let message = error.to_string();
        assert!(message.contains("cdrom"), "names what was asked: {message}");
        assert!(message.contains("hdd"), "names what is claimed: {message}");
    }

    #[test]
    fn a_device_types_name_is_not_a_slot_prefix() {
        // The two namespaces are separate deliberately, so neither
        // resolves the other's spelling.
        assert!(
            AttachmentId::parse("c15410").is_err(),
            "a device type's stable spelling is not a slot prefix"
        );
        assert!(
            DeviceType::from_id("cbmfloppy").is_err(),
            "a slot prefix is not a device type's stable spelling"
        );
        assert_eq!(
            AttachmentId::parse("cbmfloppy0").expect("parses").prefix(),
            "cbmfloppy"
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
        let slot = DeviceSlot::from(HardDrive::MbrSector);
        let device = StorageDevice::new(slot, AttachmentId::new(slot, 1));
        assert_eq!(device.attachment().to_string(), "hdd1");
        assert_eq!(
            device.device_type(),
            Some(DeviceType::HardDrive(HardDrive::MbrSector))
        );
        assert!(!device.is_occupied());
        assert_eq!(device.media_id(), None);

        let receiver = StorageDevice::new(
            DeviceSlot::Archive,
            AttachmentId::new(DeviceSlot::Archive, 0),
        );
        assert_eq!(receiver.attachment().to_string(), "arc0");
        assert_eq!(
            receiver.device_type(),
            None,
            "the receiver records nothing, as the archive it holds was \
             recorded by nothing"
        );
    }
}
