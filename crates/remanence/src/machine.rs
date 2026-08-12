// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The session and its two pools, and the machines within it (P32).
//!
//! A **session** is the outermost scope and keeps the meaning the
//! principles already give it: the P7 claims, the P27 cache budget and
//! private session storage, and the lifetime everything below it lives
//! within. It owns exactly two pools — **machines**, which are
//! configuration, and **media**, which are state — and they are
//! independent of each other by construction. A **machine** is one device
//! set inside that scope, owning its own attachment identities and its
//! own attachment order.
//!
//! **The medium is the content handle, and the pool is where media
//! live.** [`Session::load_media`] takes the caller's own opened file and
//! one declared [`Format`], answers with a [`Medium`] linked to nothing,
//! and everything the recording can answer answers there. A device is
//! configuration a medium may be *inserted* into; ejecting severs and
//! takes nothing away. That independence is the point: an archive a disk
//! was mastered out of may be released while the disk keeps answering,
//! and a reconstructed machine may be torn down without touching a
//! single disk it held.
//!
//! **A machine carries an identity, and the session's anonymous machine
//! is the one whose identity is null.** A session always has exactly one
//! of those, and it behaves as any other machine in every respect: it is
//! not "machine zero", and no attachment order it carries is more
//! meaningful than any other's. It serves the caller who is opening
//! artifacts rather than reconstructing a machine.
//!
//! **Every pool here runs the same verbs: create, look up, remove.** A
//! lookup answers with an `Option` — absence is an answer — and each
//! carries a `require_*` companion for the caller who means a demand,
//! which refuses by name and says which identity it could not find.
//! Removal names what it takes with it: [`Session::release_machine`]
//! cascades through the configuration below it and severs every link
//! naming it, [`MachineView::remove_device`] ejects first, and
//! [`Session::release_media`] is the one verb that destroys state.

use std::fs::File;
use std::path::Path;

use crate::device::AccessIntent;
use crate::device_family::DeviceFamily;
use crate::discovery::{Discovery, discover_media_with_cache};
use crate::error::{Error, Result};
use crate::media::{Format, MediaId, MediaPool, Medium};
use crate::disk::MediumState;
use crate::storage_device::{AttachmentId, DeviceView, StorageDevice};

/// An open session: the claim scope, the cache budget, the machines
/// configured within it, and the media it holds.
///
/// Every medium loaded anywhere in the session holds its own P7 claim for
/// as long as the session holds the medium — through inserts and ejects
/// alike. Dropping the session drops every machine and every medium, and
/// every claim with them.
#[derive(Debug)]
pub struct Session {
    machines: Vec<Machine>,
    media: MediaPool,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            machines: vec![Machine::anonymous()],
            media: MediaPool::default(),
        }
    }
}

impl Session {
    /// A session holding nothing but its anonymous machine. Machines,
    /// devices and media are added and removed over its life; no set is
    /// fixed at open.
    pub fn new() -> Self {
        Self::default()
    }

    // ------------------------------------------------------ the media pool

    /// Loads the caller's own opened artifact as the [`Format`] they
    /// declare it to be, and answers with the medium — **linked to
    /// nothing**.
    ///
    /// **The declaration is checked, never trusted.** Exactly one
    /// adapter is consulted — the one the format names — and a
    /// declaration the evidence cannot bear is refused naming both what
    /// was declared and what was found. Nothing probes for a second
    /// answer; a caller who does not yet know what an artifact is asks
    /// [`discover_media`](crate::discover_media), which answers that
    /// question on no handle at all.
    ///
    /// **Whoever opens owns the lock** (P7 as amended). The source is the
    /// caller's own [`File`], and that open is the claim: the library
    /// checks it for exactly one thing — may it write through it? —
    /// honours the answer exactly, and never supplements it with a lock
    /// of its own. A name is recovered from the handle for **location
    /// alone**, under an identity check, so the commit journal lands
    /// beside the artifact and a backing chain's parent is looked for
    /// next door; a handle this host cannot name serves everything else
    /// and refuses those two journeys by name.
    ///
    /// Nothing links the medium to a machine. Putting it in a drive is
    /// [`DeviceView::insert`], and it is a separate act.
    pub fn load_media(&mut self, source: File, format: Format) -> Result<&mut Medium> {
        self.load_media_with_cache(source, format, crate::DEFAULT_CACHE_BYTES)
    }

    /// [`Session::load_media`] under a caller-declared session cache
    /// bound (P27).
    pub fn load_media_with_cache(
        &mut self,
        source: File,
        format: Format,
        cache_bytes: u64,
    ) -> Result<&mut Medium> {
        let state = MediumState::load(source, format, cache_bytes)?;
        self.admit(state)
    }

    /// Loads the medium a [`Discovery`] already opened, **consuming the
    /// discovery**, and answers with the pooled medium.
    ///
    /// This is the load that runs nothing twice. A discovery holds the
    /// claim taken when the artifact was identified and the work that
    /// identification did; the state moves into the pool here, so there
    /// is no window between the question and the load in which the
    /// artifact could have changed (P7 continuity), and the intent, the
    /// cache bound and the assurance are the ones the discovery
    /// established rather than a second open's.
    pub fn load_discovery(&mut self, discovery: Discovery) -> Result<&mut Medium> {
        self.admit(discovery.into_medium())
    }

    /// Takes an opened medium into the pool, refusing an artifact this
    /// release holds in no device before it is pooled at all.
    fn admit(&mut self, state: MediumState) -> Result<&mut Medium> {
        // A flux artifact is refused whatever was declared: the block
        // reading opens anything at the raw adapter, and letting that
        // stand would declare the block layer authoritative where the
        // artifact's own adapter declares flux (P13). Discovery makes the
        // same refusal at the same depth.
        if let Some(foreign) = state.foreign_family() {
            return Err(Error::unsupported(format!(
                "{} is a {foreign}-family artifact, and a {foreign} artifact \
                 is read through its own type rather than as a block medium",
                state.named()
            )));
        }
        let id = self.media.admit(state);
        Ok(self.media.get_mut(id).expect("just admitted"))
    }

    /// Every medium the session holds, in the order they were loaded.
    pub fn media(&self) -> Vec<MediaId> {
        self.media.ids()
    }

    /// The medium `id` names, or `None` — absence is an answer, and
    /// nothing is manufactured to report it.
    pub fn medium(&self, id: MediaId) -> Option<&Medium> {
        self.media.get(id)
    }

    /// The same, ready to be worked.
    pub fn medium_mut(&mut self, id: MediaId) -> Option<&mut Medium> {
        self.media.get_mut(id)
    }

    /// **The one state-destroying verb.** It severs the medium's link if
    /// a device holds it, ends the P7 claim, and discards everything
    /// uncommitted.
    ///
    /// Severing rather than refusing keeps the model uniform with the
    /// configuration cascade: a link is configuration, and configuration
    /// never outranks the state it points at. Releasing is not a commit
    /// and never becomes one — buffered changes go with the medium, so a
    /// caller who wants them keeps them by committing first (P2).
    pub fn release_media(&mut self, id: MediaId) -> Result<()> {
        let link = self
            .media
            .get(id)
            .ok_or_else(|| Error::not_found(format!("this session holds no {id}")))?
            .link()
            .cloned();
        if let Some(link) = link {
            let machine = self
                .machine_at(link.machine.as_deref())
                .expect("a link names a machine this session holds");
            if let Some(device) = machine.device_mut(link.attachment) {
                device.set_media_id(None);
            }
        }
        self.media.take(id).map(drop)
    }

    // --------------------------------------------------- the machine pool

    /// Adds a machine carrying `identity` and answers with it.
    ///
    /// An identity already in use is refused by name rather than
    /// resolving to the machine that holds it — two machines answering
    /// to one name is a configuration error, not a lookup. The empty
    /// identity is refused too: the machine with no identity is the
    /// anonymous one, and there is exactly one of it.
    pub fn add_machine(&mut self, identity: impl Into<String>) -> Result<MachineView<'_>> {
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
        Ok(MachineView {
            machine: self.machines.last_mut().expect("just pushed"),
            pool: &mut self.media,
        })
    }

    /// Every machine in the session, the anonymous one among them, in
    /// the order they were added.
    pub fn machines(&self) -> &[Machine] {
        &self.machines
    }

    /// The machine identified `identity`, or `None` — absence is an
    /// answer, and asking never creates one.
    pub fn machine(&self, identity: &str) -> Option<&Machine> {
        self.machines
            .iter()
            .find(|machine| machine.identity() == Some(identity))
    }

    /// The same, with the media pool beside it — the borrow the verbs
    /// that cross configuration and state are spelled on.
    pub fn machine_mut(&mut self, identity: &str) -> Option<MachineView<'_>> {
        let machine = self
            .machines
            .iter_mut()
            .find(|machine| machine.identity() == Some(identity))?;
        Some(MachineView {
            machine,
            pool: &mut self.media,
        })
    }

    /// The machine identified `identity`, or the refusal naming it —
    /// the demand beside the lookup, for the caller who knows the
    /// absence is an error where they stand.
    pub fn require_machine(&mut self, identity: &str) -> Result<MachineView<'_>> {
        if self.machine(identity).is_none() {
            return Err(Error::not_found(format!(
                "this session holds no machine identified '{identity}'"
            )));
        }
        Ok(self.machine_mut(identity).expect("just looked it up"))
    }

    /// Releases a machine and everything it configures, **taking no
    /// state with it**: every device is ejected first — severing, so each
    /// medium stays pooled with its claim and its buffered changes — then
    /// the devices go, then the machine.
    ///
    /// The session's anonymous machine is not releasable: a session
    /// always has exactly one of it, and releasing it would leave the
    /// session in a shape no other verb can restore.
    pub fn release_machine(&mut self, identity: &str) -> Result<()> {
        let at = self
            .machines
            .iter()
            .position(|machine| machine.identity() == Some(identity))
            .ok_or_else(|| {
                Error::not_found(format!(
                    "this session holds no machine identified '{identity}'"
                ))
            })?;
        self.media.sever_machine(Some(identity));
        self.machines.remove(at);
        Ok(())
    }

    /// The anonymous machine — the one whose identity is null.
    pub fn anonymous(&self) -> &Machine {
        self.machines
            .iter()
            .find(|machine| machine.identity().is_none())
            .expect("a session always holds its anonymous machine")
    }

    pub fn anonymous_mut(&mut self) -> MachineView<'_> {
        let machine = self
            .machines
            .iter_mut()
            .find(|machine| machine.identity().is_none())
            .expect("a session always holds its anonymous machine");
        MachineView {
            machine,
            pool: &mut self.media,
        }
    }

    fn machine_at(&mut self, identity: Option<&str>) -> Option<&mut Machine> {
        self.machines
            .iter_mut()
            .find(|machine| machine.identity() == identity)
    }

    // ------------------------- the anonymous machine's device set, in reach

    /// Adds a device of `family` to the session's anonymous machine, as
    /// [`MachineView::add_device`] does there.
    pub fn add_device(&mut self, family: DeviceFamily) -> Result<DeviceView<'_>> {
        self.anonymous_mut().into_added_device(family, None)
    }

    /// Adds a device of `family` at the named slot of the anonymous
    /// machine, as [`MachineView::add_device_at`] does there.
    pub fn add_device_at(&mut self, family: DeviceFamily, index: u32) -> Result<DeviceView<'_>> {
        self.anonymous_mut().into_added_device(family, Some(index))
    }

    /// Adds a device for the artifact at `path` to the session's
    /// anonymous machine and loads it, as [`MachineView::add_device_for`]
    /// does there.
    pub fn add_device_for(
        &mut self,
        path: impl AsRef<Path>,
        intent: AccessIntent,
    ) -> Result<DeviceView<'_>> {
        self.add_device_for_with_cache(path, intent, crate::DEFAULT_CACHE_BYTES)
    }

    /// [`Session::add_device_for`] under a caller-declared session cache
    /// bound (P27).
    pub fn add_device_for_with_cache(
        &mut self,
        path: impl AsRef<Path>,
        intent: AccessIntent,
        cache_bytes: u64,
    ) -> Result<DeviceView<'_>> {
        let attachment = self
            .anonymous_mut()
            .add_device_for_with_cache(path, intent, cache_bytes)?
            .attachment();
        Ok(self
            .anonymous_mut()
            .into_device(attachment)
            .expect("just added"))
    }

    /// Releases the device at `attachment` from the anonymous machine,
    /// ejecting first, as [`MachineView::remove_device`] does there.
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

    /// The anonymous machine's device at `attachment`, or `None` where
    /// no device is attached there.
    pub fn device(&self, attachment: AttachmentId) -> Option<&StorageDevice> {
        self.anonymous().device(attachment)
    }

    /// The same, with the media pool beside it — the handle `insert`,
    /// `eject` and the medium answer on.
    pub fn device_mut(&mut self, attachment: AttachmentId) -> Option<DeviceView<'_>> {
        self.anonymous_mut().into_device(attachment)
    }

    /// The anonymous machine's device at `attachment`, or a refusal
    /// naming the empty slot.
    pub fn require_device(&mut self, attachment: AttachmentId) -> Result<DeviceView<'_>> {
        self.anonymous_mut().into_required_device(attachment)
    }
}

/// The device family a discovery's format declares, or the refusal that
/// says no format declared one.
///
/// The refusal names three things, because a caller who meets it has to
/// choose a drive: what the artifact is, what the medium is, and which
/// claimed families are served that medium. The last is derived from the
/// families' own declarations, so it is a list of drives the explicit
/// path will actually accept rather than a suggestion.
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
            "add the drive the machine had — {} — and insert the medium into it",
            accepting.join(", ")
        ),
    };
    Err(Error::unsupported(format!(
        "{} is a {} and that format declares no default device; it holds \
         {}, so {drives}",
        crate::media::named(discovery.path()),
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

    pub(crate) fn device_mut(&mut self, attachment: AttachmentId) -> Option<&mut StorageDevice> {
        self.position(attachment).map(|at| &mut self.devices[at])
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

/// A machine reached with its session's media pool beside it.
///
/// Configuration is the machine's and state is the session's, so the
/// verbs that touch both — adding a device you can immediately insert
/// into, tearing a machine's device down without touching its disks —
/// are spelled here, where the borrow holds both and can outlive
/// neither.
#[derive(Debug)]
pub struct MachineView<'a> {
    machine: &'a mut Machine,
    pool: &'a mut MediaPool,
}

impl<'a> MachineView<'a> {
    /// This machine's identity, or `None` where it is the session's
    /// anonymous machine.
    pub fn identity(&self) -> Option<&str> {
        self.machine.identity()
    }

    /// The machine this view names, as configuration.
    pub fn machine(&self) -> &Machine {
        self.machine
    }

    /// Adds a device of `family` in the lowest free slot of that family,
    /// and answers with the device — empty, until a medium is inserted
    /// into it.
    ///
    /// **The family must be concrete.** An interior name of the lineage
    /// classifies and never instantiates: a device added as "some floppy"
    /// declares no media an insert could be checked against and no
    /// mechanism a machine ever had, so it is refused by name (P3).
    pub fn add_device(&mut self, family: DeviceFamily) -> Result<DeviceView<'_>> {
        let index = self.machine.lowest_free_index(family);
        self.add_device_at(family, index)
    }

    /// Adds a device of `family` at the named slot.
    ///
    /// The caller chooses the **slot**, never the name: an attachment
    /// identity is always its family's slot prefix plus its index. A slot
    /// already taken is refused by name rather than displacing what is
    /// there — releasing a device is [`MachineView::remove_device`], and
    /// it is a separate act.
    pub fn add_device_at(
        &mut self,
        family: DeviceFamily,
        index: u32,
    ) -> Result<DeviceView<'_>> {
        let attachment = self.place(family, Some(index))?;
        Ok(self.device_mut(attachment).expect("just placed"))
    }

    /// Adds a fresh device of the artifact's **format-declared default
    /// family**, loads the medium into the session's pool, inserts it,
    /// and answers with that device.
    ///
    /// This is the one convenience over discovery, and it composes the
    /// three acts without changing the access path: it discovers the
    /// artifact at `path`, adds a device — a fresh one, never a slot
    /// already in the machine — pools the discovery and inserts it, so
    /// one claim is held from the question to the load (P7) and nothing
    /// expensive runs twice.
    ///
    /// **A format that declares no default refuses by name**, toward the
    /// explicit acts: a raw image says nothing about the machine it came
    /// from, and a default guessed from a media type would be the library
    /// asserting a drive nobody stated (P3). The refusal names the
    /// families the medium *could* go in, which are derived from the
    /// families' own declarations.
    pub fn add_device_for(
        &mut self,
        path: impl AsRef<Path>,
        intent: AccessIntent,
    ) -> Result<DeviceView<'_>> {
        self.add_device_for_with_cache(path, intent, crate::DEFAULT_CACHE_BYTES)
    }

    /// [`MachineView::add_device_for`] under a caller-declared session
    /// cache bound (P27), which the discovery is made under and the
    /// medium keeps.
    pub fn add_device_for_with_cache(
        &mut self,
        path: impl AsRef<Path>,
        intent: AccessIntent,
        cache_bytes: u64,
    ) -> Result<DeviceView<'_>> {
        let discovery = discover_media_with_cache(path, intent, cache_bytes)?;
        let family = default_family(&discovery)?;
        let attachment = self.place(family, None)?;
        let media = self.pool.admit(discovery.into_medium());
        let mut device = self.device_mut(attachment).expect("just placed");
        match device.insert(media) {
            Ok(()) => Ok(self.device_mut(attachment).expect("just placed")),
            Err(error) => {
                // The convenience is one act to a caller, so a refused
                // insert leaves neither a slot nor a pooled medium behind
                // for them to clean up. The explicit path is where each
                // outlives a failure, because there the caller made them
                // deliberately.
                let _ = self.pool.take(media);
                self.discard(attachment);
                Err(error)
            }
        }
    }

    /// Releases the device, **ejecting first**: the link is severed and
    /// the medium stays in the session's pool, claim and buffered changes
    /// intact. The slot is freed.
    ///
    /// Adding and releasing a device are **machine-down operations**.
    /// Nothing may be running over a device while it is reconfigured,
    /// which is exactly why the freed slot can be reused: no live state
    /// refers to the old occupant. This is not the renumbering U4 refuses
    /// for evidence-bearing lists — a slot is caller-supplied
    /// configuration, not evidence.
    ///
    /// An empty slot is refused by name: this is a removal rather than a
    /// lookup, and a caller told the device is gone learns something a
    /// silent success would hide.
    pub fn remove_device(&mut self, attachment: AttachmentId) -> Result<()> {
        let mut device = self
            .device_mut(attachment)
            .ok_or_else(|| Error::not_found(format!("no device is attached at {attachment}")))?;
        if device.is_occupied() {
            device.eject()?;
        }
        self.discard(attachment);
        Ok(())
    }

    /// Every device in this machine, in the order they were added.
    pub fn devices(&self) -> &[StorageDevice] {
        self.machine.devices()
    }

    /// The attachment identities currently in use.
    pub fn attachments(&self) -> Vec<AttachmentId> {
        self.machine.attachments()
    }

    /// The device at `attachment`, or `None` where this machine has no
    /// device there — another machine's `hdd0` is not this one's.
    pub fn device(&self, attachment: AttachmentId) -> Option<&StorageDevice> {
        self.machine.device(attachment)
    }

    /// The same, with the media pool beside it — the handle the edge
    /// verbs live on.
    pub fn device_mut(&mut self, attachment: AttachmentId) -> Option<DeviceView<'_>> {
        let machine = self.machine.identity().map(ToOwned::to_owned);
        let device = self.machine.device_mut(attachment)?;
        Some(DeviceView {
            device,
            pool: self.pool,
            machine,
        })
    }

    /// The device at `attachment`, or a refusal naming the empty slot.
    pub fn require_device(&mut self, attachment: AttachmentId) -> Result<DeviceView<'_>> {
        let machine = self.machine.identity().map(ToOwned::to_owned);
        let device = self
            .machine
            .device_mut(attachment)
            .ok_or_else(|| Error::not_found(format!("no device is attached at {attachment}")))?;
        Ok(DeviceView {
            device,
            pool: self.pool,
            machine,
        })
    }

    /// Adds a device and answers with it, consuming this view — the
    /// spelling a session-level convenience needs, where the machine
    /// borrow and the device borrow have the same life.
    fn into_added_device(
        mut self,
        family: DeviceFamily,
        index: Option<u32>,
    ) -> Result<DeviceView<'a>> {
        let attachment = self.place(family, index)?;
        Ok(self.into_device(attachment).expect("just placed"))
    }

    /// The device at `attachment`, consuming this view.
    ///
    /// The consuming form is what a caller who reached the machine
    /// through a lookup needs: the device borrow lives as long as the
    /// machine borrow did rather than nested inside it.
    pub fn into_device(self, attachment: AttachmentId) -> Option<DeviceView<'a>> {
        let machine = self.machine.identity().map(ToOwned::to_owned);
        let device = self.machine.device_mut(attachment)?;
        Some(DeviceView {
            device,
            pool: self.pool,
            machine,
        })
    }

    /// The same as [`MachineView::into_device`], refusing by name where
    /// the slot is empty — the spelling a session-level demand needs,
    /// where the machine borrow and the device borrow have one life.
    pub fn into_required_device(self, attachment: AttachmentId) -> Result<DeviceView<'a>> {
        self.into_device(attachment)
            .ok_or_else(|| Error::not_found(format!("no device is attached at {attachment}")))
    }

    /// Puts a fresh device in a slot of `family` — the caller's, or the
    /// lowest free one — and answers with its attachment identity.
    fn place(&mut self, family: DeviceFamily, index: Option<u32>) -> Result<AttachmentId> {
        if !family.is_concrete() {
            return Err(Error::unsupported(format!(
                "'{}' classifies device families and instantiates none; a \
                 machine holds a drive it actually had, and {} names a kind \
                 rather than one",
                family.id(),
                family.name()
            )));
        }
        let index = index.unwrap_or_else(|| self.machine.lowest_free_index(family));
        let attachment = AttachmentId::new(family, index);
        if self.machine.device(attachment).is_some() {
            return Err(Error::unsupported(format!(
                "{attachment} is already taken; release that device before \
                 adding another there"
            )));
        }
        self.machine.devices.push(StorageDevice::new(attachment));
        Ok(attachment)
    }

    /// Drops the device at `attachment` from the machine's set. The
    /// caller has already dealt with whatever occupied it.
    fn discard(&mut self, attachment: AttachmentId) {
        self.machine
            .devices
            .retain(|device| device.attachment() != attachment);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_session_starts_with_its_anonymous_machine_and_no_media() {
        let session = Session::new();
        assert_eq!(session.machines().len(), 1);
        assert_eq!(session.anonymous().identity(), None);
        assert!(session.media().is_empty());
    }

    #[test]
    fn a_machine_identity_is_stated_once_and_never_empty() {
        let mut session = Session::new();
        session.add_machine("pc").expect("added");
        assert!(session.add_machine("pc").is_err(), "one name, one machine");
        assert!(
            session.add_machine("").is_err(),
            "the machine with no identity is the anonymous one"
        );
        assert!(session.machine("pc").is_some());
        assert!(session.machine("laptop").is_none(), "absence is an answer");
    }

    #[test]
    fn a_released_machine_takes_its_configuration_and_nothing_else() {
        let mut session = Session::new();
        {
            let mut pc = session.add_machine("pc").expect("added");
            pc.add_device(DeviceFamily::HARD_DISK).expect("added");
        }
        session.release_machine("pc").expect("released");
        assert!(session.machine("pc").is_none());
        assert!(
            session.release_machine("pc").is_err(),
            "the second release names what is no longer there"
        );
    }

    #[test]
    fn an_interior_family_name_instantiates_no_device() {
        // P3, one rung up the lineage: a machine holds a drive it had.
        let mut session = Session::new();
        let error = session
            .add_device(DeviceFamily::FLOPPY_DRIVE)
            .expect_err("refused");
        assert!(
            error.to_string().contains("classifies"),
            "names why: {error}"
        );
    }
}
