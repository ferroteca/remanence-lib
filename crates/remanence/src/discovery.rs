// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! Discovery: what an artifact is, before a machine has been configured
//! for it.
//!
//! [`discover_media`] is a **first-class library function, on no handle
//! at all**. It needs no session and no machine because it consults
//! catalogs and evidence rather than configuration: it claims the
//! artifact for the read (P7), identifies it, and answers with the exact
//! article, the device types the recognizing format records, and the
//! devices that would accept the article. It mutates nothing (P2).
//!
//! **The discovery it answers with is a consumable handle — a claim
//! scope holding the work already done.** Recognizing an artifact is not
//! free, and re-opening it to load it would do that work twice and, worse,
//! leave a window between the question and the load in which the file
//! could change. So [`crate::Session::load_discovery`] takes the
//! discovery and moves its state into the media pool: one open, one
//! claim, held continuously from the question to the load (P7
//! continuity).
//!
//! **The library opens here, so P7's mandatory denial applies in full.**
//! Discovery names an artifact by path — a caller who does not yet know
//! what something is has no handle-and-format declaration to make — and
//! the claim is the library's own. That is the other half of the amended
//! rule whose first half [`crate::Session::load_media`] carries.
//!
//! **The device type is the image format's declaration, not the
//! article's** (P12). An article cannot honestly carry it — a ten-sector
//! hard-sectored 5.25-inch disk is the substrate of more than one
//! machine's drive — while the format that records one ecosystem's disk
//! can say so, and does, by admitting exactly one type. The devices that
//! *accept* the article are the other direction entirely, derived by
//! asking the device catalog (P14, D19).
//!
//! **A format that records several device types asserts none here**, and
//! that is the honest answer rather than a deficiency: a qcow2 was
//! written by some hard drive and nothing in the artifact says which. A
//! discovery still reports what it is; loading it into the pool, and the
//! convenience over it, refuse by name toward the declared load, which
//! is where the caller states the type (P3).

use std::path::Path;

use crate::assurance::Assurance;
use crate::device::{AccessIntent, AccessMode};
use crate::device_type::{DeviceSlot, DeviceType};
use crate::disk::{DiskFormat, MediumState};
use crate::error::{Error, Result};
use crate::session::Identification;

/// Identifies the artifact at `path` — a disk image, or an archive —
/// under the caller's declared intent, and answers with what it is and
/// where it could go.
///
/// A path names a file. An artifact *inside* an archive is discovered
/// through the namespace its archive bears, by
/// [`crate::File::discover`], under the claim the archive already
/// holds.
///
/// The claim taken here is held by the returned [`Discovery`] until it is
/// consumed or dropped, so a `Write` discovery claims the artifact
/// exclusively exactly as a load does, and a discovery that cannot secure
/// its claim fails here rather than falling back (P7).
///
/// Discovery answers a question; it configures nothing. Adding a device
/// is [`crate::MachineView::add_device`], loading a medium is
/// [`crate::Session::load_media`], and the one convenience that composes
/// the acts over a discovery is
/// [`crate::MachineView::add_device_for`].
pub fn discover_media(path: impl AsRef<Path>, intent: AccessIntent) -> Result<Discovery> {
    discover_media_with_cache(path, intent, crate::DEFAULT_CACHE_BYTES)
}

/// [`discover_media`] under a caller-declared session cache bound (P27).
/// The bound travels into the device with the discovery, so a load that
/// consumes one keeps the bound the discovery was made under.
pub fn discover_media_with_cache(
    path: impl AsRef<Path>,
    intent: AccessIntent,
    cache_bytes: u64,
) -> Result<Discovery> {
    let path = path.as_ref();
    let medium = MediumState::open(path, intent, cache_bytes)?;
    if let Some(foreign) = medium.foreign_family() {
        return Err(Error::unsupported(format!(
            "'{}' is a {foreign}-family artifact, and this release \
             discovers no {foreign} medium; a {foreign} artifact is \
             loaded by its own declaration at `load_media`",
            path.display()
        )));
    }
    Ok(Discovery { medium })
}

/// What one artifact turned out to be, and the claim under which that
/// was established.
///
/// It is a handle rather than a record because it holds two things a
/// record could not: the P7 claim on the artifact, and the work the
/// recognition did. Everything it reports is a value — identities and
/// names — and reporting them mutates nothing.
///
/// A discovery is **consumed by a load** and dropped otherwise; either
/// way the claim ends there. Dropping one is not a refusal of anything,
/// and asking again is always allowed — it is only the work and the
/// continuity that are lost.
#[derive(Debug)]
pub struct Discovery {
    medium: MediumState,
}

impl Discovery {
    /// The artifact claimed — the archive itself for an image
    /// discovered inside one, which is where the claim sits.
    ///
    /// Absent where the artifact was reached through a handle this host
    /// cannot name, which a discovery over an archive entry inherits
    /// from the archive it came out of.
    pub fn path(&self) -> Option<&str> {
        self.medium.path()
    }

    /// The resolved artifact — the entry name for an image discovered
    /// inside an archive, else the source's own name.
    pub fn image_path(&self) -> Option<&Path> {
        self.medium.image_path()
    }

    /// The resolved image's own size in bytes — the raw plane, distinct
    /// from [`Discovery::size`], which is the disk the format presents.
    pub fn image_size_bytes(&self) -> u64 {
        self.medium.image_size_bytes()
    }

    /// The presented disk's size (the guest-visible size for qcow2), or
    /// the refusal naming a medium that presents no disk.
    ///
    /// An archive has a namespace and no space, so it answers here by
    /// name; its artifact's own extent is
    /// [`Discovery::image_size_bytes`], which every medium has.
    pub fn size(&self) -> Result<u64> {
        Ok(self.medium.space("size")?.size())
    }

    /// The image container format the artifact turned out to be, or the
    /// refusal naming a medium that is no disk image. An archive's
    /// grammar is [`Discovery::image_format`].
    pub fn format(&self) -> Result<DiskFormat> {
        Ok(self.medium.space("format")?.format())
    }

    /// The recognized format's stable spelling — `h8d`, `qcow2`, `vdi`,
    /// `raw`, or an archive grammar's `zip` or `7z`.
    pub fn image_format(&self) -> &'static str {
        self.medium.format_id()
    }

    /// That format's name, fit to show a user.
    pub fn image_format_name(&self) -> &'static str {
        self.medium.format_name()
    }

    /// The **exact article**, by the catalog's stable spelling (P14).
    /// The image-format adapter that loaded the state named it; nothing
    /// here guessed.
    pub fn article(&self) -> &'static str {
        self.medium.media().id
    }

    /// The article's name, fit to show a user beside the drive it goes
    /// in.
    pub fn article_name(&self) -> &'static str {
        self.medium.media().name
    }

    /// The device this artifact's content was recorded by, where the
    /// recognizing format admits exactly one — and `None` where it
    /// records several and nothing in the artifact says which.
    ///
    /// A load takes this answer as it stands: with a type, the discovery
    /// pools as a medium that knows what wrote it; without one, the pool
    /// refuses by name toward the declared load
    /// ([`crate::Session::load_media`]).
    pub fn device_type(&self) -> Option<DeviceType> {
        self.medium.device_type()
    }

    /// Every device type the recognizing format records — one where it
    /// carries it bare, several where a load declares which, and none
    /// for an archive grammar, which records no device at all.
    pub fn device_types(&self) -> &'static [DeviceType] {
        self.medium.recorded_devices()
    }

    /// Every device an artifact of this article could go in, derived
    /// from the device catalog's own declarations rather than from a
    /// second list.
    ///
    /// It is the answer to "where could this go?", which is a different
    /// question from [`Discovery::device_type`]'s "what wrote it?". An
    /// empty answer means no device this release claims is served the
    /// article — an archive's case, and the honest one.
    pub fn accepting_devices(&self) -> Vec<DeviceType> {
        DeviceType::accepting(self.medium.media())
    }

    /// What slot a load of this artifact would go into: the recording
    /// device where one is known, the archive receiver for an archive,
    /// and `None` where the format records several types and the load
    /// must declare which.
    pub fn device_slot(&self) -> Option<DeviceSlot> {
        self.medium.slot()
    }

    /// The **effective** access mode this discovery established: the
    /// declared intent's echo where the evidence supports it, read-only
    /// where it does not (P28). A load that consumes the discovery
    /// inherits it, because it is the same open.
    pub fn mode(&self) -> AccessMode {
        self.medium.mode()
    }

    /// What the open established about the evidence beneath the medium
    /// (P28) — available before anything is read, and carried into the
    /// device by a load that consumes this discovery.
    pub fn assurance(&self) -> &Assurance {
        self.medium.assurance()
    }

    /// Identifies the artifact's nesting layers and probable
    /// filesystem, over bounded evidence alone (P27) — the same reading
    /// [`crate::Medium::identify`] gives once a medium is loaded.
    pub fn identify(&self) -> Identification {
        self.medium.identify()
    }

    /// The state behind this discovery, for the refusals a load makes
    /// about it before taking it.
    pub(crate) fn state(&self) -> &MediumState {
        &self.medium
    }

    /// The medium, taken out of the discovery by the load that consumes
    /// it. The claim moves with the state; nothing is re-opened.
    pub(crate) fn into_medium(self) -> MediumState {
        self.medium
    }

    /// A discovery over a medium already opened — the nested journey,
    /// where the artifact was reached through a namespace rather than
    /// named by path.
    pub(crate) fn over(medium: MediumState) -> Self {
        Self { medium }
    }
}
