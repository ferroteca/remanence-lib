// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The storage-device tier (P32): a session holds a set of family-typed
//! devices, and a device is a durable slot distinct from whatever medium
//! currently occupies it.
//!
//! The device is the slot, not the disk. Ejecting a medium and attaching
//! another leaves the device where it was, which is what makes this a tier
//! rather than a rename of the medium surface F43 delivered.

use std::fmt;

use crate::disk::Disk;
use crate::error::{Error, Result};

/// The family a storage device belongs to.
///
/// A family is an enumerated claim (P3). Only the block family is claimed
/// today, so `hdd0` is real and `floppy0` is not yet; a name outside the
/// claim is refused by name rather than guessed at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeviceFamily {
    /// Block-addressed fixed storage — the family the library already
    /// reads.
    Hdd,
}

impl DeviceFamily {
    /// The family's stable name, and the prefix of every attachment
    /// identity in it.
    pub fn name(self) -> &'static str {
        match self {
            Self::Hdd => "hdd",
        }
    }

    /// Resolves a family name, refusing one the library does not claim.
    ///
    /// Rust callers hold the enum and cannot express an unclaimed family;
    /// this exists for the C and Python surfaces, where a family arrives
    /// as text and the P3 refusal has to happen at that boundary.
    pub fn from_name(name: &str) -> Result<Self> {
        match name {
            "hdd" => Ok(Self::Hdd),
            other => Err(Error::unsupported(format!(
                "no storage-device family named '{other}' is claimed; \
                 this release claims 'hdd'"
            ))),
        }
    }
}

impl fmt::Display for DeviceFamily {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

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
    pub(crate) fn new(family: DeviceFamily, index: u32) -> Self {
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
    pub fn parse(text: &str) -> Result<Self> {
        let split = text
            .find(|c: char| c.is_ascii_digit())
            .ok_or_else(|| Error::unsupported(format!(
                "'{text}' is not an attachment identity; one reads like 'hdd0'"
            )))?;
        let (family, index) = text.split_at(split);
        let family = DeviceFamily::from_name(family)?;
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
        write!(f, "{}{}", self.family.name(), self.index)
    }
}

/// One storage device: a durable, family-typed slot holding zero or one
/// attached medium.
///
/// The medium is the disk currently inserted; the device outlives it.
#[derive(Debug)]
pub struct StorageDevice {
    attachment: AttachmentId,
    medium: Option<Disk>,
}

impl StorageDevice {
    pub(crate) fn new(attachment: AttachmentId, medium: Option<Disk>) -> Self {
        Self { attachment, medium }
    }

    /// This device's attachment identity — `hdd0` and the like.
    pub fn attachment(&self) -> AttachmentId {
        self.attachment
    }

    pub fn family(&self) -> DeviceFamily {
        self.attachment.family()
    }

    /// Whether a medium is currently attached.
    pub fn is_occupied(&self) -> bool {
        self.medium.is_some()
    }

    /// The attached medium, or `None` when the slot is empty.
    pub fn medium(&self) -> Option<&Disk> {
        self.medium.as_ref()
    }

    /// The attached medium for the verbs that need it mutably — inspect,
    /// the file verbs, commit and rollback.
    pub fn medium_mut(&mut self) -> Option<&mut Disk> {
        self.medium.as_mut()
    }

    /// The attached medium, or a refusal naming the empty slot.
    pub fn require_medium(&mut self) -> Result<&mut Disk> {
        let attachment = self.attachment;
        self.medium.as_mut().ok_or_else(|| {
            Error::not_found(format!("no medium is attached to {attachment}"))
        })
    }

    /// Removes the attached medium, releasing its P7 claim, and leaves the
    /// device in place.
    pub(crate) fn eject(&mut self) -> Option<Disk> {
        self.medium.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_attachment_identity_reads_as_family_and_slot() {
        let id = AttachmentId::new(DeviceFamily::Hdd, 0);
        assert_eq!(id.to_string(), "hdd0");
        assert_eq!(AttachmentId::parse("hdd0").expect("parses"), id);
        assert_eq!(
            AttachmentId::parse("hdd12").expect("parses").index(),
            12,
            "a slot is not a single digit"
        );
    }

    #[test]
    fn an_unclaimed_family_is_refused_by_name() {
        // P3: the family set is an enumerated claim. `floppy` is the
        // family P32 names next, and naming it here must refuse rather
        // than pretend, because no floppy medium can be attached yet.
        let error = DeviceFamily::from_name("floppy").expect_err("refused");
        let message = error.to_string();
        assert!(message.contains("floppy"), "names what was asked: {message}");
        assert!(message.contains("hdd"), "names what is claimed: {message}");

        let error = AttachmentId::parse("floppy0").expect_err("refused");
        assert!(error.to_string().contains("floppy"), "refuses through parse too");
    }

    #[test]
    fn a_malformed_identity_is_refused_rather_than_guessed() {
        assert!(AttachmentId::parse("hdd").is_err(), "no slot at all");
        assert!(AttachmentId::parse("").is_err(), "empty");
        assert!(AttachmentId::parse("0").is_err(), "no family");
    }
}
