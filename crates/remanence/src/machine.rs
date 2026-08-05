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
use crate::device_family::DeviceFamily;
use crate::discovery::{Discovery, discover_media_with_cache};
use crate::error::{Error, Result};
use crate::storage_device::{AttachmentId, StorageDevice};

/// An open session: the claim scope, the cache budget, and the machines
/// within it.
///
/// Every medium loaded anywhere in the session holds its own P7 claim for
/// as long as it stays in its device. Dropping the session drops every
/// machine, and every medium and claim with them.
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

    /// Adds a device of `family` to the session's anonymous machine, as
    /// [`Machine::add_device`] does there.
    pub fn add_device(&mut self, family: DeviceFamily) -> Result<&mut StorageDevice> {
        self.anonymous_mut().add_device(family)
    }

    /// Adds a device for the artifact at `path` to the session's
    /// anonymous machine, as [`Machine::add_device_for`] does there.
    pub fn add_device_for(
        &mut self,
        path: impl AsRef<Path>,
        intent: AccessIntent,
    ) -> Result<&mut StorageDevice> {
        self.anonymous_mut().add_device_for(path, intent)
    }

    /// [`Session::add_device_for`] under a caller-declared session cache
    /// bound (P27).
    pub fn add_device_for_with_cache(
        &mut self,
        path: impl AsRef<Path>,
        intent: AccessIntent,
        cache_bytes: u64,
    ) -> Result<&mut StorageDevice> {
        self.anonymous_mut()
            .add_device_for_with_cache(path, intent, cache_bytes)
    }

    /// Adds a device of `family` at the named slot of the anonymous
    /// machine, as [`Machine::add_device_at`] does there.
    pub fn add_device_at(
        &mut self,
        family: DeviceFamily,
        index: u32,
    ) -> Result<&mut StorageDevice> {
        self.anonymous_mut().add_device_at(family, index)
    }

    /// Removes the device at `attachment` from the anonymous machine.
    pub fn remove_device(&mut self, attachment: AttachmentId) -> Result<()> {
        self.anonymous_mut().remove_device(attachment)
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

/// The device family a discovery's format declares, or the refusal that
/// says no format declared one.
///
/// The refusal names three things, because a caller who meets it has to
/// choose a drive: what the artifact is, what the medium is, and which
/// claimed families are served that medium. The last is derived from the
/// families' own declarations, so it is a list of drives the two-act path
/// will actually accept rather than a suggestion.
fn default_family(discovery: &Discovery) -> Result<DeviceFamily> {
    if let Some(family) = discovery.default_device() {
        return Ok(family);
    }
    let accepting: Vec<&str> = discovery
        .accepting_families()
        .iter()
        .map(|family| family.id())
        .collect();
    let drives = match accepting.len() {
        0 => "no drive family this release claims is served that medium".to_owned(),
        _ => format!(
            "add the drive the machine had — {} — and load the medium into it",
            accepting.join(", ")
        ),
    };
    Err(Error::unsupported(format!(
        "'{}' is a {} and that format declares no default device; it holds \
         {}, so {drives}",
        discovery.path(),
        discovery.image_format_name(),
        discovery.media_type_name()
    )))
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

    /// Adds a device of `family` in the lowest free slot of that family,
    /// and answers with the device — empty, until something is loaded
    /// into it.
    ///
    /// **The family must be concrete.** An interior name of the lineage
    /// classifies and never instantiates: a device added as "some floppy"
    /// declares no media a load could be checked against and no mechanism
    /// a machine ever had, so it is refused by name (P3).
    pub fn add_device(&mut self, family: DeviceFamily) -> Result<&mut StorageDevice> {
        let index = self.lowest_free_index(family);
        self.add_device_at(family, index)
    }

    /// Adds a device of `family` at the named slot.
    ///
    /// The caller chooses the **slot**, never the name: an attachment
    /// identity is always its family's slot prefix plus its index. A slot
    /// already taken is refused by name rather than displacing what is
    /// there — removing a device is [`Machine::remove_device`], and it is
    /// a separate act.
    pub fn add_device_at(
        &mut self,
        family: DeviceFamily,
        index: u32,
    ) -> Result<&mut StorageDevice> {
        if !family.is_concrete() {
            return Err(Error::unsupported(format!(
                "'{}' classifies device families and instantiates none; a \
                 machine holds a drive it actually had, and {} names a kind \
                 rather than one",
                family.id(),
                family.name()
            )));
        }

        let attachment = AttachmentId::new(family, index);
        if self.position(attachment).is_some() {
            return Err(Error::unsupported(format!(
                "{attachment} is already taken; remove that device before \
                 adding another there"
            )));
        }

        self.devices.push(StorageDevice::new(attachment));
        Ok(self.devices.last_mut().expect("just pushed"))
    }

    /// Adds a fresh device of the artifact's **format-declared default
    /// family**, loads the medium into it, and answers with that device.
    ///
    /// This is the one convenience over discovery, and it composes the
    /// two acts without changing the access path: it discovers the
    /// artifact at `path`, adds a device — a fresh one, never a slot
    /// already in the machine — and consumes the discovery into it, so
    /// one claim is held from the question to the load (P7) and nothing
    /// expensive runs twice.
    ///
    /// **A format that declares no default refuses by name**, toward the
    /// two explicit acts: a raw image says nothing about the machine it
    /// came from, and a default guessed from a media type would be the
    /// library asserting a drive nobody stated (P3). The refusal names
    /// the families the medium *could* go in, which are derived from the
    /// families' own declarations.
    ///
    /// There is no media-first spelling of this. With one storage handle
    /// both spellings would return the same device.
    pub fn add_device_for(
        &mut self,
        path: impl AsRef<Path>,
        intent: AccessIntent,
    ) -> Result<&mut StorageDevice> {
        self.add_device_for_with_cache(path, intent, crate::DEFAULT_CACHE_BYTES)
    }

    /// [`Machine::add_device_for`] under a caller-declared session cache
    /// bound (P27), which the discovery is made under and the device
    /// keeps.
    pub fn add_device_for_with_cache(
        &mut self,
        path: impl AsRef<Path>,
        intent: AccessIntent,
        cache_bytes: u64,
    ) -> Result<&mut StorageDevice> {
        let discovery = discover_media_with_cache(path, intent, cache_bytes)?;
        let family = default_family(&discovery)?;
        let attachment = self.add_device(family)?.attachment();
        match self
            .require_device(attachment)
            .expect("the device was added a statement ago")
            .load_discovery(discovery)
        {
            Ok(()) => self.require_device(attachment),
            Err(error) => {
                // The convenience is one act to a caller, so a refused
                // load leaves no slot behind for them to clean up. The
                // two-act path is where a device outlives a failed load,
                // because there the caller made the device deliberately.
                self.remove_device(attachment)
                    .expect("the device was added two statements ago");
                Err(error)
            }
        }
    }

    /// Removes the device, releasing any medium's P7 claim with it and
    /// freeing its slot.
    ///
    /// Adding and removing a device are **machine-down operations**.
    /// Nothing may be running over a device while it is reconfigured,
    /// which is exactly why the freed slot can be reused: no live state
    /// refers to the old occupant. This is not the renumbering U4 refuses
    /// for evidence-bearing lists — a slot is caller-supplied
    /// configuration, not evidence.
    pub fn remove_device(&mut self, attachment: AttachmentId) -> Result<()> {
        let position = self
            .position(attachment)
            .ok_or_else(|| Error::not_found(format!("no device is attached at {attachment}")))?;
        let mut device = self.devices.remove(position);
        drop(device.take_medium());
        Ok(())
    }

    /// Every device in this machine, in the order they were added — the
    /// attachment order a namespace composer reads.
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
