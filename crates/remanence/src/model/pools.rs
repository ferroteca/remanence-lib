// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The session: its device set and its media pool (P32).
//!
//! A **session** is the outermost scope and keeps the meaning the
//! principles already give it: the P7 claims, the P27 cache budget and
//! private session storage, and the lifetime everything below it lives
//! within. Within it sit **devices**, which are configuration, and
//! **media**, which are state, and they are independent of each other by
//! construction.
//!
//! **The medium is the content handle, and the pool is where media
//! live.** [`Session::load_media`] takes the caller's own opened file and
//! one declared [`Format`], answers with a [`Medium`] linked to nothing,
//! and everything the recording can answer answers there. A device is
//! configuration a medium may be *inserted* into; ejecting severs and
//! takes nothing away. That independence is the point: an archive a disk
//! was mastered out of may be released while the disk keeps answering,
//! and a drive may be torn down without touching the disk it held.
//!
//! **The session is the device scope, and there is no machine object.**
//! The attachment identities — `hdd0`, `fd0` — are the session's, and so
//! is the order they were added in. A tier grouping devices into named
//! machines is *proposed* rather than built: it earns itself where
//! artifacts nest, one machine holding a host's archive and another the
//! disk inside it, and this release resolves nesting one level deep and
//! reads no device set as a set, so building the tier now would be
//! structure ahead of the demand that has to shape it.
//!
//! **Both halves run the same three verbs: create, look up, release.** A
//! lookup answers with an `Option` — absence is an answer, and nothing is
//! manufactured to report it — so there is no `require_*` form and a
//! caller who wants a demand writes it, at the place that knows what the
//! demand means. Creation still refuses by name (a taken slot), because
//! that is the world saying no rather than a lookup finding nothing. And
//! the removals are both spelled `release_*`:
//! [`Session::release_device`] ejects first, and
//! [`Session::release_media`] severs its own link and then ends the
//! claim.

use std::path::Path;

use crate::error::{Error, Result};
use crate::io::device::AccessIntent;
use crate::model::authored::NewMedia;
use crate::model::device_type::{DeviceSlot, DeviceType};
use crate::model::discovery::{Discovery, discover_media};
use crate::model::disk::{MediumRecognition, MediumState};
use crate::model::media::{Format, MediaId, MediaPool, MediaSource, Medium};
use crate::model::storage_device::{AttachmentId, DeviceView, StorageDevice};

/// An open session: the claim scope, the cache budget, the devices
/// configured within it, and the media it holds.
///
/// Every medium loaded in the session holds its own P7 claim for as long
/// as the session holds the medium — through inserts and ejects alike.
/// Dropping the session drops every device and every medium, and every
/// claim with them.
#[derive(Debug, Default)]
pub struct Session {
    devices: Vec<StorageDevice>,
    media: MediaPool,
}

impl Session {
    /// A session holding nothing. Devices and media are added and removed
    /// over its life; no set is fixed at open.
    pub fn new() -> Self {
        Self::default()
    }

    // ------------------------------------------------------ the media pool

    /// Loads a declared source as the [`Format`] the caller declares it
    /// to be, and answers with the medium — **linked to nothing**.
    ///
    /// **The source takes one of four shapes** (each arriving by plain
    /// conversion into [`MediaSource`]): the caller's own opened
    /// [`std::fs::File`], a collection of them, one
    /// [`FileSource`](crate::FileSource) taken from another medium's
    /// namespace, or a collection of those. A format declares which
    /// shape it reads, as it declares everything else — a KryoFlux
    /// capture set is a declared collection, every other claimed format
    /// one artifact — and a shape the format does not read is refused by
    /// name.
    ///
    /// **The declaration is checked, never trusted.** Exactly one
    /// adapter is consulted — the one the format names — and a
    /// declaration the evidence cannot bear is refused naming both what
    /// was declared and what was found. Nothing probes for a second
    /// answer; a caller who does not yet know what an artifact is asks
    /// [`discover_media`](crate::discover_media), which answers that
    /// question on no handle at all.
    ///
    /// **Whoever opens owns the lock** (P7 as amended). A local
    /// artifact arrives as the caller's own [`std::fs::File`], and that open is
    /// the claim: the library checks it for exactly one thing — may it
    /// write through it? — honours the answer exactly, and never
    /// supplements it with a lock of its own. A name is recovered from
    /// the handle for **location alone**, under an identity check, so
    /// the commit journal lands beside the artifact and a backing
    /// chain's parent is looked for next door; a handle this host
    /// cannot name serves everything else and refuses those two
    /// journeys by name. A file from another medium's namespace rides
    /// that medium's claim instead, and a KryoFlux member's name is the
    /// member grammar's own evidence — the one record of a stream's
    /// position — so a nameless member refuses the collection by name.
    ///
    /// Nothing links the medium to a machine. Putting it in a drive is
    /// [`DeviceView::insert`], and it is a separate act.
    pub fn load_media(
        &mut self,
        source: impl Into<MediaSource>,
        format: Format,
    ) -> Result<&mut Medium> {
        self.load_media_with_cache(source, format, crate::DEFAULT_CACHE_BYTES)
    }

    /// [`Session::load_media`] under a caller-declared session cache
    /// bound (P27).
    pub fn load_media_with_cache(
        &mut self,
        source: impl Into<MediaSource>,
        format: Format,
        cache_bytes: u64,
    ) -> Result<&mut Medium> {
        let state = MediumState::load(source.into(), format, cache_bytes)?;
        self.admit(state)
    }

    /// Loads the medium a [`Discovery`] already opened, **consuming the
    /// discovery**, and answers with the pooled medium.
    ///
    /// This is the load that runs nothing twice. A discovery holds the
    /// claim taken when the artifact was identified and the work that
    /// identification did; the medium is built over that claim here, so
    /// there is no window between the question and the load in which the
    /// artifact could have changed (P7 continuity), and the intent and
    /// the assurance are the ones the discovery established rather than a
    /// second open's.
    ///
    /// **The cache bound is declared here**, because this is where the
    /// medium comes into existence: discovery built nothing, so it had
    /// nothing to bound (P27).
    ///
    /// **This is the plain door, and it opens where the recognizing
    /// format records exactly one device type** — an H8D is a Heathkit
    /// H-17 recording, an archive was recorded by nothing. Where the
    /// format records several, nothing in the artifact says which wrote
    /// it, and the refusal names them and points at
    /// [`Session::load_discovery_as`], where the caller declares one.
    /// It is the same pair the vantage doors make: the plain door where
    /// the evidence determines the answer, the `_as` form where the
    /// caller's reading does.
    pub fn load_discovery(&mut self, discovery: Discovery) -> Result<&mut Medium> {
        self.load_discovery_with_cache(discovery, crate::DEFAULT_CACHE_BYTES)
    }

    /// [`Session::load_discovery`] under a caller-declared session cache
    /// bound (P27).
    pub fn load_discovery_with_cache(
        &mut self,
        discovery: Discovery,
        cache_bytes: u64,
    ) -> Result<&mut Medium> {
        self.admit(discovery.into_medium(cache_bytes))
    }

    /// [`Session::load_discovery`] under the caller's own declaration of
    /// the device that recorded the artifact.
    ///
    /// It is the declared reading of a discovery, and the library checks
    /// it: the type must be one the recognizing format's adapter records,
    /// so a raw member of an archive may be declared a hard-drive
    /// recording and never a 1541's. Declaring where the format already
    /// carries the type is allowed and must agree; the plain door is
    /// what a caller with nothing to add uses.
    pub fn load_discovery_as(
        &mut self,
        discovery: Discovery,
        device: DeviceType,
    ) -> Result<&mut Medium> {
        self.load_discovery_as_with_cache(discovery, device, crate::DEFAULT_CACHE_BYTES)
    }

    /// [`Session::load_discovery_as`] under a caller-declared session
    /// cache bound (P27).
    pub fn load_discovery_as_with_cache(
        &mut self,
        mut discovery: Discovery,
        device: DeviceType,
        cache_bytes: u64,
    ) -> Result<&mut Medium> {
        let recorded = discovery.device_types();
        if !recorded.contains(&device) {
            return Err(Error::unsupported(format!(
                "{} is a {} and that format records {}, so a declaration \
                 naming '{}' names a recording it never made",
                crate::model::media::named(discovery.path()),
                discovery.image_format_name(),
                match recorded.len() {
                    0 => "no device at all".to_owned(),
                    _ => recorded
                        .iter()
                        .map(|device| device.id())
                        .collect::<Vec<_>>()
                        .join(" and "),
                },
                device.id()
            )));
        }
        discovery.declare_device(device);
        self.admit(discovery.into_medium(cache_bytes))
    }

    /// Creates blank media whole — **authorship, the third fact class**
    /// — and answers with the medium, linked to nothing.
    ///
    /// Nothing is discovered here, because there is no artifact: the
    /// author declares one enumerated [`NewMedia`] kind, and the facts
    /// that declaration states become the medium's original facts,
    /// carried from creation as its [`assurance`](Medium::assurance)
    /// provenance and — where the kind states coordinates — as its
    /// [`geometry`](Medium::geometry), whose one reading is authorship.
    ///
    /// **A declaration names a concrete catalog entry** (P3), as every
    /// creation verb here does: the **blank article kinds**, each naming
    /// one article of the P14 catalog and creating that manufactured
    /// substrate with nothing recorded on it, and
    /// [`NewMedia::ChsDisk`], whose article is authored rather than
    /// manufactured and whose content is addressed in the cylinders,
    /// heads and sectors the author stated. Coordinates that address
    /// nothing are refused when they are stated, which is the one moment
    /// authorship offers to check them.
    ///
    /// **An authored blank assumes no device.**
    /// [`device_type`](Medium::device_type) answers `None` — the same
    /// honest absence an archive answers — so no drive takes one and
    /// [`DeviceView::insert`] refuses by name. Recording a layout onto a
    /// blank article ([`PartitionView::record_as`](crate::PartitionView::record_as))
    /// is what binds one.
    ///
    /// The medium is **session-backed**: its content lives in the
    /// session's own bounded working set (P27) until an explicit encode
    /// gives it an artifact, and [`Medium::commit`] is the ordinary
    /// commit point over it (P2).
    pub fn new_media(&mut self, kind: NewMedia) -> Result<&mut Medium> {
        self.new_media_with_cache(kind, crate::DEFAULT_CACHE_BYTES)
    }

    /// [`Session::new_media`] under a caller-declared session cache
    /// bound (P27), which is what bounds the authored content's resident
    /// working set.
    pub fn new_media_with_cache(
        &mut self,
        kind: NewMedia,
        cache_bytes: u64,
    ) -> Result<&mut Medium> {
        let state = MediumState::authored(kind, cache_bytes)?;
        self.admit(state)
    }

    /// Takes an opened or authored medium into the pool, refusing an
    /// artifact this release holds in no device before it is pooled at
    /// all, and establishing its partition pool in the same act.
    ///
    /// **A pooled medium can always say what recorded it.** A discovery
    /// over a format that records several device types asserts none, and
    /// it is refused here rather than pooled as a medium that could be
    /// neither seated nor laid out — the caller declares the type at
    /// [`Session::load_discovery_as`], or at
    /// [`Session::load_media`] where they hold the file (P3).
    fn admit(&mut self, mut state: MediumState) -> Result<&mut Medium> {
        // A flux artifact is refused whatever was declared: the block
        // reading opens anything at the raw adapter, and letting that
        // stand would declare the block layer authoritative where the
        // artifact's own adapter declares flux (P13). Discovery makes the
        // same refusal at the same depth.
        if let Some(foreign) = state.foreign_family() {
            return Err(Error::unsupported(format!(
                "{} is a {foreign}-family artifact, and a {foreign} artifact \
                 is loaded by its own declaration rather than as a block \
                 medium: declare the format it is at `load_media`",
                state.named()
            )));
        }
        // A *reading* that could not say what recorded it is refused. An
        // authored medium says none either, and there is nothing missing
        // about it — the author assumed no device — so the question the
        // pool asks is which of the two this is.
        if state.undeclared() {
            return Err(undeclared_device(&state));
        }
        // The evidence pool is established here, once, before anyone holds
        // the medium: the scheme its device type's spec declares is
        // checked against the content, and what the check answers is what
        // the pool bears for the session's life (F56, F57).
        let partitions = state.establish_partitions()?;
        // The geometry is established in the same act and for the same
        // reason: it is discovered evidence about the artifact, read
        // once, before anyone holds the medium, and never declared onto
        // it afterwards (F58).
        let geometry = state.establish_geometry(&partitions);
        let id = self.media.admit(state, partitions, geometry);
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
        if let Some(link) = link
            && let Some(device) = self.device_mut_raw(link.attachment)
        {
            device.set_media_id(None);
        }
        self.media.take(id).map(drop)
    }

    // ---------------------------------------------------- the device set

    /// Adds a device in the lowest free slot of its bay, and answers
    /// with it — empty, until a medium is inserted into it.
    ///
    /// **A device is as concrete as the machine fact it asserts**: one
    /// device type, or the archive receiver. There is no vaguer thing to
    /// pass — "some floppy" is not a value, because a device type the
    /// library does not know fails to compile — and the type is what the
    /// insert check weighs a medium's recording against (P3).
    pub fn add_device(&mut self, slot: impl Into<DeviceSlot>) -> Result<DeviceView<'_>> {
        let slot = slot.into();
        let index = self.lowest_free_index(slot);
        self.add_device_at(slot, index)
    }

    /// Adds a device at the named slot.
    ///
    /// The caller chooses the **slot**, never the name: an attachment
    /// identity is always its bay's prefix plus its index. A slot already
    /// taken is refused by name rather than displacing what is there —
    /// releasing a device is [`Session::release_device`], and it is a
    /// separate act.
    pub fn add_device_at(
        &mut self,
        slot: impl Into<DeviceSlot>,
        index: u32,
    ) -> Result<DeviceView<'_>> {
        let attachment = self.place(slot.into(), Some(index))?;
        Ok(self.device_mut(attachment).expect("just placed"))
    }

    /// Adds a fresh device of the **device type the artifact's format
    /// records**, loads the medium into the session's pool, inserts it,
    /// and answers with that device.
    ///
    /// This is the one convenience over discovery, and it composes the
    /// three acts without changing the access path: it discovers the
    /// artifact at `path`, adds a device — a fresh one, never a slot
    /// already in the session — pools the discovery and inserts it, so
    /// one claim is held from the question to the load (P7) and nothing
    /// expensive runs twice.
    ///
    /// **A format that records several device types refuses by name**,
    /// toward the declared load: nothing in a qcow2 says which hard
    /// drive wrote it, and a type guessed from the article would be the
    /// library asserting a drive nobody stated (P3). The refusal names
    /// the types the adapter records, which is exactly what a
    /// declaration may say.
    pub fn add_device_for(
        &mut self,
        path: impl AsRef<Path>,
        intent: AccessIntent,
    ) -> Result<DeviceView<'_>> {
        self.add_device_for_with_cache(path, intent, crate::DEFAULT_CACHE_BYTES)
    }

    /// [`Session::add_device_for`] under a caller-declared session cache
    /// bound (P27). The bound belongs to the load half of this
    /// convenience — the discovery it opens with creates nothing and has
    /// nothing to bound — and the medium it makes keeps it.
    pub fn add_device_for_with_cache(
        &mut self,
        path: impl AsRef<Path>,
        intent: AccessIntent,
        cache_bytes: u64,
    ) -> Result<DeviceView<'_>> {
        let discovery = discover_media(path, intent)?;
        let slot = discovered_slot(&discovery)?;
        let attachment = self.place(slot, None)?;
        let mut state = discovery.into_medium(cache_bytes);
        let partitions = match state.establish_partitions() {
            Ok(partitions) => partitions,
            Err(error) => {
                self.discard(attachment);
                return Err(error);
            }
        };
        let geometry = state.establish_geometry(&partitions);
        let media = self.media.admit(state, partitions, geometry);
        let mut device = self.device_mut(attachment).expect("just placed");
        match device.insert(media) {
            Ok(()) => Ok(self.device_mut(attachment).expect("just placed")),
            Err(error) => {
                // The convenience is one act to a caller, so a refused
                // insert leaves neither a slot nor a pooled medium behind
                // for them to clean up. The explicit path is where each
                // outlives a failure, because there the caller made them
                // deliberately.
                let _ = self.media.take(media);
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
    pub fn release_device(&mut self, attachment: AttachmentId) -> Result<()> {
        let mut device = self
            .device_mut(attachment)
            .ok_or_else(|| Error::not_found(format!("no device is attached at {attachment}")))?;
        if device.is_occupied() {
            device.eject()?;
        }
        self.discard(attachment);
        Ok(())
    }

    /// Every device in this session, in the order they were added.
    pub fn devices(&self) -> &[StorageDevice] {
        &self.devices
    }

    /// The attachment identities currently in use.
    pub fn attachments(&self) -> Vec<AttachmentId> {
        self.devices.iter().map(StorageDevice::attachment).collect()
    }

    /// The device at `attachment`, or `None` where nothing is attached
    /// there — absence is an answer, and asking never creates one.
    pub fn device(&self, attachment: AttachmentId) -> Option<&StorageDevice> {
        self.position(attachment).map(|at| &self.devices[at])
    }

    /// The same, with the media pool beside it — the handle `insert`,
    /// `eject` and the medium answer on, since linking is the one act
    /// that crosses configuration into state.
    pub fn device_mut(&mut self, attachment: AttachmentId) -> Option<DeviceView<'_>> {
        let at = self.position(attachment)?;
        Some(DeviceView {
            device: &mut self.devices[at],
            pool: &mut self.media,
        })
    }

    fn device_mut_raw(&mut self, attachment: AttachmentId) -> Option<&mut StorageDevice> {
        self.position(attachment).map(|at| &mut self.devices[at])
    }

    fn position(&self, attachment: AttachmentId) -> Option<usize> {
        self.devices
            .iter()
            .position(|device| device.attachment() == attachment)
    }

    /// The lowest free slot in the bay `slot` fills.
    ///
    /// It counts by **bay, not by device type**: two hard drives of
    /// different types cannot both be `hdd0`, because a slot is a place
    /// in the machine and only one drive sits in it.
    fn lowest_free_index(&self, slot: DeviceSlot) -> u32 {
        let mut used: Vec<u32> = self
            .devices
            .iter()
            .filter(|device| device.attachment().prefix() == slot.slot_prefix())
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

    /// Puts a fresh device in a slot — the caller's, or the lowest free
    /// one in its bay — and answers with its attachment identity.
    fn place(&mut self, slot: DeviceSlot, index: Option<u32>) -> Result<AttachmentId> {
        let index = index.unwrap_or_else(|| self.lowest_free_index(slot));
        let attachment = AttachmentId::new(slot, index);
        if self.device(attachment).is_some() {
            return Err(Error::unsupported(format!(
                "{attachment} is already taken; release that device before \
                 adding another there"
            )));
        }
        self.devices.push(StorageDevice::new(slot, attachment));
        Ok(attachment)
    }

    /// Drops the device at `attachment` from the set. The caller has
    /// already dealt with whatever occupied it.
    fn discard(&mut self, attachment: AttachmentId) {
        self.devices
            .retain(|device| device.attachment() != attachment);
    }
}

/// The refusal a medium that cannot say what recorded it meets, at the
/// pool's door and at the convenience alike.
///
/// It names three things, because a caller who meets it has to make a
/// declaration: what the artifact turned out to be, which device types
/// its format records, and that the declared load is where one of them
/// is stated. The list is the adapter's own, so it is exactly what a
/// declaration will be accepted for rather than a suggestion.
fn undeclared_device(state: &MediumState) -> Error {
    undeclared(state.named(), state.format_name(), state.recorded_devices())
}

/// The same refusal about a reading that has not been materialized:
/// what a discovery meets at the convenience over it.
fn undeclared_recognition(recognized: &MediumRecognition) -> Error {
    undeclared(
        recognized.named(),
        recognized.format_name(),
        recognized.recorded_devices(),
    )
}

fn undeclared(named: String, format_name: &str, devices: &[DeviceType]) -> Error {
    let recorded: Vec<&str> = devices.iter().map(|device| device.id()).collect();
    Error::unsupported(format!(
        "{named} is a {format_name} and that format records {} — nothing in \
         the artifact says which wrote it. Declare one at the load: \
         `load_discovery_as` over this discovery, or `load_media` where \
         the artifact is a file you hold.",
        match recorded.len() {
            0 => "no device at all".to_owned(),
            _ => recorded.join(" and "),
        }
    ))
}

/// The slot a discovery's artifact goes in, or the refusal that says the
/// format records several device types and named none.
fn discovered_slot(discovery: &Discovery) -> Result<DeviceSlot> {
    discovery
        .device_slot()
        .ok_or_else(|| undeclared_recognition(discovery.recognized()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::device_type::HardDrive;

    #[test]
    fn a_session_starts_with_no_devices_and_no_media() {
        let session = Session::new();
        assert!(session.devices().is_empty());
        assert!(session.media().is_empty());
        assert!(session.attachments().is_empty());
    }

    #[test]
    fn one_bay_holds_one_drive_whatever_type_it_is() {
        // A slot is a place in the machine, so the lowest free one counts
        // by bay: two hard drives of different types cannot both be
        // hdd0, and the second takes hdd1.
        let mut session = Session::new();
        assert_eq!(
            session
                .add_device(HardDrive::MbrSector)
                .expect("added")
                .attachment()
                .to_string(),
            "hdd0"
        );
        assert_eq!(
            session
                .add_device(HardDrive::MbrBlock)
                .expect("added")
                .attachment()
                .to_string(),
            "hdd1"
        );
        assert!(
            session.add_device_at(HardDrive::Gpt, 0).is_err(),
            "the bay is taken, whatever drive would go in it"
        );
        assert_eq!(
            session
                .add_device(DeviceSlot::Archive)
                .expect("added")
                .attachment()
                .to_string(),
            "arc0",
            "a receiver fills its own bay, not the drives'"
        );
    }

    /// A released slot is reusable because adding and releasing are
    /// machine-down operations: no live state refers to the old occupant.
    #[test]
    fn a_released_slot_is_free_again_and_an_empty_one_refuses() {
        let mut session = Session::new();
        session.add_device(HardDrive::MbrSector).expect("added");
        let hdd0 = AttachmentId::new(HardDrive::MbrSector.into(), 0);
        session.release_device(hdd0).expect("released");
        assert!(session.devices().is_empty(), "the slot is free again");
        assert!(
            session.release_device(hdd0).is_err(),
            "a removal names what is no longer there"
        );
        assert_eq!(
            session
                .add_device(HardDrive::MbrSector)
                .expect("added")
                .attachment()
                .to_string(),
            "hdd0",
            "the freed bay is the lowest free one again"
        );
    }
}
