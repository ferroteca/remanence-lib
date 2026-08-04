// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The session: the machine scope (P32).
//!
//! There is no separate machine object. A session *is* the scope within
//! which device identity is resolved, and it holds a dynamic set of
//! family-typed storage devices. Nothing above it groups several sessions
//! into one machine, and nothing needs to.

use std::path::Path;

use crate::device::AccessIntent;
use crate::disk::Disk;
use crate::error::{Error, Result};
use crate::storage_device::{AttachmentId, DeviceFamily, StorageDevice};

/// An open session over one machine's storage.
///
/// The session holds the devices; each device holds at most one medium,
/// and each medium holds its own P7 claim for as long as it is attached.
/// Dropping the session detaches everything and releases every claim.
#[derive(Debug, Default)]
pub struct Session {
    devices: Vec<StorageDevice>,
}

impl Session {
    /// A session with no devices. Devices are attached and detached over
    /// its life; the set is not fixed at open.
    pub fn new() -> Self {
        Self::default()
    }

    /// Attaches the medium at `path` to a new device, taking the lowest
    /// free slot in its family, and returns the attachment identity the
    /// device took.
    ///
    /// The family is settled by what the medium turns out to be. Only the
    /// block family is claimed today, so this is always an `hdd` slot.
    pub fn attach(&mut self, path: impl AsRef<Path>, intent: AccessIntent) -> Result<AttachmentId> {
        self.attach_with_cache(path, intent, crate::DEFAULT_CACHE_BYTES)
    }

    /// Attaches the medium at `path` to the named slot.
    ///
    /// The caller chooses the **slot**, never the name: an attachment
    /// identity is always its family plus its index. A slot already
    /// occupied is refused by name rather than displacing what is there —
    /// ejecting is [`Session::detach`], and it is a separate act.
    pub fn attach_at(
        &mut self,
        family: DeviceFamily,
        index: u32,
        path: impl AsRef<Path>,
        intent: AccessIntent,
    ) -> Result<AttachmentId> {
        self.attach_at_with_cache(family, index, path, intent, crate::DEFAULT_CACHE_BYTES)
    }

    /// Attaches to the lowest free slot under a caller-declared session
    /// cache bound (P27), as [`Session::attach`] otherwise does.
    pub fn attach_with_cache(
        &mut self,
        path: impl AsRef<Path>,
        intent: AccessIntent,
        cache_bytes: u64,
    ) -> Result<AttachmentId> {
        let family = DeviceFamily::Hdd;
        let index = self.lowest_free_index(family);
        self.attach_at_with_cache(family, index, path, intent, cache_bytes)
    }

    /// Attaches to a named slot under a caller-declared session cache
    /// bound (P27), as [`Session::attach_at`] otherwise does.
    pub fn attach_at_with_cache(
        &mut self,
        family: DeviceFamily,
        index: u32,
        path: impl AsRef<Path>,
        intent: AccessIntent,
        cache_bytes: u64,
    ) -> Result<AttachmentId> {
        let attachment = AttachmentId::new(family, index);
        if self.position(attachment).is_some() {
            return Err(Error::unsupported(format!(
                "{attachment} is already occupied; detach it before attaching another medium"
            )));
        }

        let medium = Disk::open_with_cache(path.as_ref(), intent, cache_bytes)?;

        // A device accepts only its own family's media (P14). This is
        // where that bites, and it is not idle even with one family
        // claimed: the block catalog opens anything it cannot identify at
        // the raw adapter, so without this a flux container would be
        // admitted to a block device and read as raw — declaring the
        // block layer authoritative when its own adapter declares flux,
        // which in-force P13 forbids.
        if let Some(foreign) = foreign_family(&medium) {
            return Err(Error::unsupported(format!(
                "'{}' is a {foreign}-family artifact and {attachment} is a {} device; \
                 no {foreign} device family is claimed by this release",
                path.as_ref().display(),
                family.name()
            )));
        }

        self.devices
            .push(StorageDevice::new(attachment, Some(medium)));
        Ok(attachment)
    }

    /// Detaches the device, releasing its medium's P7 claim and freeing
    /// its slot.
    ///
    /// Attach and detach are **machine-down operations**. Nothing may be
    /// running over a device while it is reconfigured, which is exactly
    /// why the freed slot can be reused: no live state refers to the old
    /// occupant. This is not the renumbering U4 refuses for
    /// evidence-bearing lists — a slot is caller-supplied configuration,
    /// not evidence.
    pub fn detach(&mut self, attachment: AttachmentId) -> Result<()> {
        let position = self.position(attachment).ok_or_else(|| {
            Error::not_found(format!("no device is attached at {attachment}"))
        })?;
        let mut device = self.devices.remove(position);
        drop(device.eject());
        Ok(())
    }

    /// Every attached device, in the order the slots were filled.
    pub fn devices(&self) -> &[StorageDevice] {
        &self.devices
    }

    /// The attachment identities currently in use.
    pub fn attachments(&self) -> Vec<AttachmentId> {
        self.devices.iter().map(StorageDevice::attachment).collect()
    }

    pub fn device(&self, attachment: AttachmentId) -> Option<&StorageDevice> {
        self.position(attachment).map(|at| &self.devices[at])
    }

    pub fn device_mut(&mut self, attachment: AttachmentId) -> Option<&mut StorageDevice> {
        self.position(attachment).map(|at| &mut self.devices[at])
    }

    /// The device at `attachment`, or a refusal naming the empty slot.
    pub fn require_device(&mut self, attachment: AttachmentId) -> Result<&mut StorageDevice> {
        self.position(attachment)
            .map(|at| &mut self.devices[at])
            .ok_or_else(|| {
                Error::not_found(format!("no device is attached at {attachment}"))
            })
    }

    /// The medium in the device at `attachment` — the usual way in to the
    /// inspection report and the file verbs.
    pub fn medium(&mut self, attachment: AttachmentId) -> Result<&mut Disk> {
        self.require_device(attachment)?.require_medium()
    }

    fn position(&self, attachment: AttachmentId) -> Option<usize> {
        self.devices
            .iter()
            .position(|device| device.attachment() == attachment)
    }

    fn lowest_free_index(&self, family: DeviceFamily) -> u32 {
        let mut used: Vec<u32> = self
            .devices
            .iter()
            .filter(|device| device.family() == family)
            .map(|device| device.attachment().index())
            .collect();
        used.sort_unstable();
        let mut next = 0;
        for index in used {
            if index == next {
                next += 1;
            } else if index > next {
                break;
            }
        }
        next
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
fn foreign_family(medium: &Disk) -> Option<&'static str> {
    let mut prefix = [0u8; 8];
    if medium.read_at(0, &mut prefix).is_err() {
        return None;
    }
    crate::p64::has_signature(&prefix).then_some("flux")
}
