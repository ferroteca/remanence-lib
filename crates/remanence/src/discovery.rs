// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! Discovery: what an artifact is, before a machine has been configured
//! for it.
//!
//! [`discover_media`] is a **first-class library function, on no handle
//! at all**. It needs no session and no machine because it consults
//! catalogs and evidence rather than configuration: it claims the
//! artifact for the read (P7), identifies it, and answers with the exact
//! medium, the concrete device families that would accept it, and the
//! image format's declared default device. It mutates nothing (P2).
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
//! **The default device is the image format's declaration, not the
//! medium's** (P12). A medium cannot honestly carry it — a ten-sector
//! hard-sectored 5.25-inch disk is the article of more than one machine's
//! drive — while the format that records one ecosystem's disk can say so.
//! The families that *accept* the medium are the other direction entirely,
//! derived by asking the families themselves (P14, D19). A format
//! declaring no default is ordinary rather than deficient, and the
//! convenience over it refuses by name toward the two explicit acts (P3).

use std::path::Path;

use crate::assurance::Assurance;
use crate::device::{AccessIntent, AccessMode};
use crate::device_family::DeviceFamily;
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
            "'{}' is a {foreign}-family artifact and no device in this \
             release holds a {foreign} medium; a {foreign} artifact is \
             read through its own type",
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

    /// The **exact medium**, by the media-type catalog's stable spelling
    /// (P14). The image-format adapter that loaded the state named it;
    /// nothing here guessed.
    pub fn media_type(&self) -> &'static str {
        self.medium.media().id
    }

    /// The medium's name, fit to show a user beside the drive it goes in.
    pub fn media_type_name(&self) -> &'static str {
        self.medium.media().name
    }

    /// Every concrete device family a device could hold this medium in,
    /// derived from the families' own declarations rather than from a
    /// second list.
    ///
    /// It is the answer to "where could this go?", which is a different
    /// question from [`Discovery::default_device`]'s "where did this come
    /// from?". An empty answer means no drive this release claims is
    /// served the article.
    pub fn accepting_families(&self) -> Vec<DeviceFamily> {
        DeviceFamily::accepting(self.medium.media())
    }

    /// The device family the **image format** declares for the disks it
    /// records, or `None` where it declares none.
    ///
    /// `None` is ordinary: a raw image says nothing about its machine.
    /// The caller then states the drive itself, which is the two-act path
    /// and always available.
    pub fn default_device(&self) -> Option<DeviceFamily> {
        self.medium.default_device()
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
