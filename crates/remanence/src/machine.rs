// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The session and the machines within it (P32).
//!
//! A **session** is the outermost scope and keeps the meaning the
//! principles already give it: the P7 claims, the P27 cache budget and
//! private session storage, and the lifetime everything below it lives
//! within. A **machine** is one device set inside that scope, owning its
//! own attachment identities and its own attachment order.
//!
//! A session holds machines; a machine holds devices. Machines in a
//! session do not know about each other — an archive on the host was
//! never part of the machine whose disk it contains — while the session
//! owning every machine's lifetime is what lets one machine's device be
//! backed by state another machine holds, with no lifetime question
//! between them.
//!
//! **A machine carries an identity, and the session's anonymous machine
//! is the one whose identity is null.** A session always has exactly one
//! of those, and it behaves as any other machine in every respect: it is
//! not "machine zero", and no attachment order it carries is more
//! meaningful than any other's. It serves the caller who is opening
//! artifacts rather than reconstructing a machine.

use std::path::Path;

use crate::device::AccessIntent;
use crate::disk::MediaState;
use crate::error::{Error, Result};
use crate::storage_device::{AttachmentId, DeviceFamily, StorageDevice};

/// An open session: the claim scope, the cache budget, and the machines
/// within it.
///
/// Every medium attached anywhere in the session holds its own P7 claim
/// for as long as it stays attached. Dropping the session drops every
/// machine, detaching everything and releasing every claim.
#[derive(Debug)]
pub struct Session {
    machines: Vec<Machine>,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            machines: vec![Machine::anonymous()],
        }
    }
}

impl Session {
    /// A session holding nothing but its anonymous machine. Machines and
    /// devices are added and removed over its life; neither set is fixed
    /// at open.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a machine carrying `identity` and answers with it.
    ///
    /// An identity already in use is refused by name rather than
    /// resolving to the machine that holds it — two machines answering
    /// to one name is a configuration error, not a lookup. The empty
    /// identity is refused too: the machine with no identity is the
    /// anonymous one, and there is exactly one of it.
    pub fn add_machine(&mut self, identity: impl Into<String>) -> Result<&mut Machine> {
        let identity = identity.into();
        if identity.is_empty() {
            return Err(Error::unsupported(
                "a machine identity may not be empty; the machine with no \
                 identity is the session's anonymous one"
                    .to_owned(),
            ));
        }
        if self.machine(&identity).is_some() {
            return Err(Error::unsupported(format!(
                "this session already holds a machine identified '{identity}'"
            )));
        }
        self.machines.push(Machine::named(identity));
        Ok(self.machines.last_mut().expect("just pushed"))
    }

    /// Every machine in the session, the anonymous one among them, in
    /// the order they were added.
    pub fn machines(&self) -> &[Machine] {
        &self.machines
    }

    pub fn machine(&self, identity: &str) -> Option<&Machine> {
        self.machines
            .iter()
            .find(|machine| machine.identity() == Some(identity))
    }

    pub fn machine_mut(&mut self, identity: &str) -> Option<&mut Machine> {
        self.machines
            .iter_mut()
            .find(|machine| machine.identity() == Some(identity))
    }

    /// The machine identified `identity`, or a refusal naming it.
    pub fn require_machine(&mut self, identity: &str) -> Result<&mut Machine> {
        self.machine_mut(identity).ok_or_else(|| {
            Error::not_found(format!(
                "this session holds no machine identified '{identity}'"
            ))
        })
    }

    /// The anonymous machine — the one whose identity is null.
    pub fn anonymous(&self) -> &Machine {
        self.machines
            .iter()
            .find(|machine| machine.identity().is_none())
            .expect("a session always holds its anonymous machine")
    }

    pub fn anonymous_mut(&mut self) -> &mut Machine {
        self.machines
            .iter_mut()
            .find(|machine| machine.identity().is_none())
            .expect("a session always holds its anonymous machine")
    }

    /// Attaches the medium at `path` to a new device in the session's
    /// anonymous machine, as [`Machine::attach`] does there.
    pub fn attach(&mut self, path: impl AsRef<Path>, intent: AccessIntent) -> Result<AttachmentId> {
        self.anonymous_mut().attach(path, intent)
    }

    /// Attaches the medium at `path` to the named slot of the anonymous
    /// machine, as [`Machine::attach_at`] does there.
    pub fn attach_at(
        &mut self,
        family: DeviceFamily,
        index: u32,
        path: impl AsRef<Path>,
        intent: AccessIntent,
    ) -> Result<AttachmentId> {
        self.anonymous_mut().attach_at(family, index, path, intent)
    }

    /// [`Session::attach`] under a caller-declared session cache bound
    /// (P27).
    pub fn attach_with_cache(
        &mut self,
        path: impl AsRef<Path>,
        intent: AccessIntent,
        cache_bytes: u64,
    ) -> Result<AttachmentId> {
        self.anonymous_mut()
            .attach_with_cache(path, intent, cache_bytes)
    }

    /// [`Session::attach_at`] under a caller-declared session cache bound
    /// (P27).
    pub fn attach_at_with_cache(
        &mut self,
        family: DeviceFamily,
        index: u32,
        path: impl AsRef<Path>,
        intent: AccessIntent,
        cache_bytes: u64,
    ) -> Result<AttachmentId> {
        self.anonymous_mut()
            .attach_at_with_cache(family, index, path, intent, cache_bytes)
    }

    /// Detaches the device at `attachment` from the anonymous machine.
    pub fn detach(&mut self, attachment: AttachmentId) -> Result<()> {
        self.anonymous_mut().detach(attachment)
    }

    /// The anonymous machine's devices, in the order its slots were
    /// filled.
    pub fn devices(&self) -> &[StorageDevice] {
        self.anonymous().devices()
    }

    /// The attachment identities in use in the anonymous machine.
    pub fn attachments(&self) -> Vec<AttachmentId> {
        self.anonymous().attachments()
    }

    pub fn device(&self, attachment: AttachmentId) -> Option<&StorageDevice> {
        self.anonymous().device(attachment)
    }

    pub fn device_mut(&mut self, attachment: AttachmentId) -> Option<&mut StorageDevice> {
        self.anonymous_mut().device_mut(attachment)
    }

    /// The anonymous machine's device at `attachment`, or a refusal
    /// naming the empty slot — the way in to the inspection report and
    /// the file verbs.
    pub fn require_device(&mut self, attachment: AttachmentId) -> Result<&mut StorageDevice> {
        self.anonymous_mut().require_device(attachment)
    }
}

/// One machine within a session: a set of family-typed storage devices,
/// their attachment identities, and the order they were attached in.
///
/// The device set is the machine's own. Two machines in one session may
/// each hold an `hdd0`, and neither can reach the other's.
#[derive(Debug)]
pub struct Machine {
    /// Null for the session's anonymous machine.
    identity: Option<String>,
    devices: Vec<StorageDevice>,
}

impl Machine {
    fn anonymous() -> Self {
        Self {
            identity: None,
            devices: Vec::new(),
        }
    }

    fn named(identity: String) -> Self {
        Self {
            identity: Some(identity),
            devices: Vec::new(),
        }
    }

    /// This machine's identity, or `None` where it is the session's
    /// anonymous machine.
    pub fn identity(&self) -> Option<&str> {
        self.identity.as_deref()
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
    /// ejecting is [`Machine::detach`], and it is a separate act.
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
    /// cache bound (P27), as [`Machine::attach`] otherwise does.
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
    /// bound (P27), as [`Machine::attach_at`] otherwise does.
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

        let medium = MediaState::open_with_cache(path.as_ref(), intent, cache_bytes)?;

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
        let position = self
            .position(attachment)
            .ok_or_else(|| Error::not_found(format!("no device is attached at {attachment}")))?;
        let mut device = self.devices.remove(position);
        drop(device.eject());
        Ok(())
    }

    /// Every device in this machine, in the order the slots were filled —
    /// the attachment order a namespace composer reads.
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
            .ok_or_else(|| Error::not_found(format!("no device is attached at {attachment}")))
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
fn foreign_family(medium: &MediaState) -> Option<&'static str> {
    let mut prefix = [0u8; 8];
    if medium.read_at(0, &mut prefix).is_err() {
        return None;
    }
    crate::p64::has_signature(&prefix).then_some("flux")
}
