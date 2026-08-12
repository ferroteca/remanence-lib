// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The device-type catalog (P14): one identity per medium naming the
//! device its content is assumed recorded by.
//!
//! **The catalog speaks in device types, and it is a hierarchy in two
//! levels**: the *class* — [`DeviceType::Floppy`] and
//! [`DeviceType::HardDrive`] today, with `Optical` and `Tape` reserved
//! for the coming families — and the *concrete type* within it.
//! `Commodore1541` is a [`FloppyDrive`], and is a [`DeviceType`]. A type
//! the library does not know **fails to compile**; the display strings
//! (`c1541`, `mbr-block-hd`) survive in provenance, refusals, and the C
//! and Python surfaces, where the identity crosses as its stable
//! spelling — the same currency every other catalog here crosses in
//! (P5).
//!
//! **The granularity rule cuts the catalog**: a device type is the
//! coarsest name fixing the whole addressing surface and recording
//! discipline without per-media parameters. What the device fixes lives
//! in the type; what varies disk to disk lives on the medium. That is
//! why the hard-drive class carries the partition scheme in the spec —
//! the scheme is part of what the machine's controller and firmware fix
//! — and why the generic sector floppy carries no geometry, which is
//! per-media and arrives as discovered evidence, never as a type fact.
//! The two halves of that cut meet at the sector verbs: a type declares
//! **that** it is addressed by cylinder, head and sector
//! ([`DeviceType::addressing`]), and the medium's own
//! [`Geometry`](crate::Geometry) is what says how many of each — one
//! declared, the other discovered, and neither standing in for the
//! other.
//!
//! **The definition has one home: one spec shape per class, one instance
//! per concrete type.** The enumeration is the instantiation — each
//! variant resolves to exactly one static spec below, its disciplines
//! flat attributes of the profile. The *traits live on the medium*: the
//! actions (`read_blocks`, `put_sector`, `partition`) take shape as
//! surfaces on [`crate::Medium`], each answering only where the
//! profile's attribute holds, with the P30 declarations reached through
//! the type rather than passed as arguments.
//!
//! **D19's three facts keep their three homes.** The article's passive
//! facts stay in the article catalog
//! ([`crate::model::media_profile`]) — the spec *composes* an article, it never
//! restates one — the recording lives here in the device type, and the
//! drive's behavior stays in the P30 drive profile the flux path names.
//!
//! Archives were recorded by no device, so a medium answers its device
//! type as an `Option` and `None` is the honest answer — which is also
//! why no entry here describes the receiver slot a machine seats an
//! archive in: a receiver has no mechanism, and a catalog of recording
//! devices has nothing to say about it.

use std::fmt;

use crate::error::{Error, Result};
use crate::flux::drive_profile::{self, DriveProfile};
use crate::model::media_profile::{
    FLEXIBLE_5_25_HARD_10, FLEXIBLE_5_25_SOFT, LOGICAL_BLOCK_512, MediaProfile,
};
use crate::partition::PartitionScheme;

/// The concrete types of the floppy class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FloppyDrive {
    /// The Commodore 1541 — the flux product class, whose encoding,
    /// speed zones, timings and track set are the C1541 drive profile's
    /// declarations (P30), reached through this type.
    Commodore1541,
    /// The Heathkit H-17 — the hard-sectored Heathkit product class,
    /// served the ten-sector article whose holes divide the revolution.
    HeathH17,
    /// The Heathkit H-37 — the soft-sectored Heathkit product class,
    /// served the same soft-sectored article a 1541 is.
    HeathH37,
    /// The generic schemeless sector floppy: sector-addressed recording
    /// with geometry per-media — discovered evidence, never a type fact.
    Sector,
}

/// The concrete types of the hard-drive class, whose specs carry the
/// partition scheme itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HardDrive {
    /// An MBR-partitioned drive addressed by cylinder, head and sector.
    MbrSector,
    /// An MBR-partitioned drive addressed by logical block.
    MbrBlock,
    /// A GPT-partitioned drive — block-addressed by GPT's own
    /// definition. Enumerated, and loadable by no format this release
    /// claims: no adapter declares the pairing, so declaring it is a
    /// named refusal at the load rather than a reading of the table.
    Gpt,
}

/// One device type: the device a medium's content is assumed recorded
/// by, enumerated in two levels — the class, then the concrete type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeviceType {
    Floppy(FloppyDrive),
    HardDrive(HardDrive),
}

/// How a type addresses its recording: by the recording's own cylinder,
/// head and sector, or by logical block number.
///
/// It is part of the addressing surface the granularity rule says a
/// device type fixes, and it is what decides whether a medium answers
/// the sector verbs at all — the geometry those verbs address *in* is
/// discovered evidence, and this is the type's own declaration that
/// there are coordinates to discover.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Addressing {
    Sector,
    Block,
}

impl Addressing {
    const fn name(self) -> &'static str {
        match self {
            Self::Sector => "sector",
            Self::Block => "block",
        }
    }
}

/// The scheme a hard-drive spec declares its partitions laid out under.
///
/// It is deliberately not [`PartitionScheme`], which enumerates the
/// schemes this release *reads*: GPT is declared by the `gpt-hd` spec
/// and read by nothing yet, and putting it into the evidence vocabulary
/// would claim a reader that does not exist (P3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeclaredScheme {
    Mbr,
    Gpt,
}

impl DeclaredScheme {
    const fn name(self) -> &'static str {
        match self {
            Self::Mbr => "mbr",
            Self::Gpt => "gpt",
        }
    }
}

/// The spec shape of the floppy class — one instance per concrete type,
/// its disciplines flat attributes.
struct FloppySpec {
    id: &'static str,
    name: &'static str,
    provenance: &'static str,
    /// The article this type is served (P14). Composed, never restated:
    /// the article's own facts stay in the article catalog.
    article: &'static MediaProfile,
    /// How this type addresses its recording. Every floppy in the
    /// catalog is sector-addressed — a floppy drive steps to a track and
    /// reads records around it, which is what a cylinder, head and
    /// sector name — and *what* geometry each disk was recorded under
    /// stays per-media, discovered rather than declared here.
    addressing: Addressing,
    /// The family half of every attachment identity a device of this
    /// type answers to. Types may share one — two Heathkit controllers
    /// fill the same kind of slot — because a slot names a place, not a
    /// recording.
    slot_prefix: &'static str,
    /// The drive profile governing this type's recording path where it
    /// claims one (P22, P30). A type claiming none is ordinary: the H-17
    /// records ten hard-sectored records per track and this release
    /// claims no flux path for it.
    flux_path: Option<&'static DriveProfile>,
}

/// The spec shape of the hard-drive class — the partition scheme is part
/// of the spec, which is what lets every partition pool populate
/// kind-determined with no separate scheme declaration.
struct HardDriveSpec {
    id: &'static str,
    name: &'static str,
    provenance: &'static str,
    article: &'static MediaProfile,
    slot_prefix: &'static str,
    scheme: DeclaredScheme,
    addressing: Addressing,
}

static C1541_SPEC: FloppySpec = FloppySpec {
    id: "c1541",
    name: "Commodore 1541",
    provenance: "declared from the same published 1541 conventions the drive \
                 profile is: a single-surface drive served soft-sectored \
                 5.25-inch media, with a claimed physical recording path",
    article: &FLEXIBLE_5_25_SOFT,
    slot_prefix: "cbmfloppy",
    addressing: Addressing::Sector,
    flux_path: Some(&drive_profile::C1541),
};

static H17_SPEC: FloppySpec = FloppySpec {
    id: "h17",
    name: "Heathkit H-17 floppy drive",
    provenance: "declared from the published H17 conventions the H8D image \
                 format is written against: a single-surface drive served \
                 ten-sector hard-sectored 5.25-inch media, the medium's own \
                 holes dividing the revolution",
    article: &FLEXIBLE_5_25_HARD_10,
    slot_prefix: "heathfloppy",
    addressing: Addressing::Sector,
    flux_path: None,
};

static H37_SPEC: FloppySpec = FloppySpec {
    id: "h37",
    name: "Heathkit H-37 floppy drive",
    provenance: "declared from the published H37 conventions: the soft-sectored \
                 Heathkit controller, served the same 5.25-inch soft-sectored \
                 article a 1541 is and recording it in its own discipline — \
                 which is why a 1541 refuses an H37 disk it could physically \
                 hold but never serve",
    article: &FLEXIBLE_5_25_SOFT,
    slot_prefix: "heathfloppy",
    addressing: Addressing::Sector,
    flux_path: None,
};

static SECTOR_FLOPPY_SPEC: FloppySpec = FloppySpec {
    id: "sector-floppy",
    name: "sector floppy drive",
    provenance: "declared from what the granularity rule leaves when no product \
                 class is asserted: sector-addressed flexible recording with \
                 geometry per-media, the geometry arriving as discovered \
                 evidence rather than as a fact of the type",
    article: &FLEXIBLE_5_25_SOFT,
    slot_prefix: "floppy",
    addressing: Addressing::Sector,
    flux_path: None,
};

static MBR_SECTOR_HD_SPEC: HardDriveSpec = HardDriveSpec {
    id: "mbr-sector-hd",
    name: "MBR sector-addressed hard drive",
    provenance: "declared from the published MBR conventions and the CHS \
                 command set the scheme grew up on: a drive addressed by \
                 cylinder, head and sector, its partitions laid out under \
                 the MBR table its spec carries",
    article: &LOGICAL_BLOCK_512,
    slot_prefix: "hdd",
    scheme: DeclaredScheme::Mbr,
    addressing: Addressing::Sector,
};

static MBR_BLOCK_HD_SPEC: HardDriveSpec = HardDriveSpec {
    id: "mbr-block-hd",
    name: "MBR block-addressed hard drive",
    provenance: "declared from the published MBR conventions over logical \
                 block addressing: a drive addressed by block number, its \
                 partitions laid out under the MBR table its spec carries",
    article: &LOGICAL_BLOCK_512,
    slot_prefix: "hdd",
    scheme: DeclaredScheme::Mbr,
    addressing: Addressing::Block,
};

static GPT_HD_SPEC: HardDriveSpec = HardDriveSpec {
    id: "gpt-hd",
    name: "GPT hard drive",
    provenance: "declared from the published GPT specification, which defines \
                 the scheme over logical block addressing — GPT implies block \
                 addressing by its own definition, so no sector-addressed \
                 sibling exists to enumerate",
    article: &LOGICAL_BLOCK_512,
    slot_prefix: "hdd",
    scheme: DeclaredScheme::Gpt,
    addressing: Addressing::Block,
};

impl FloppyDrive {
    fn spec(self) -> &'static FloppySpec {
        match self {
            Self::Commodore1541 => &C1541_SPEC,
            Self::HeathH17 => &H17_SPEC,
            Self::HeathH37 => &H37_SPEC,
            Self::Sector => &SECTOR_FLOPPY_SPEC,
        }
    }

    /// The drive profile governing this type's recording path, where it
    /// claims one — the declaration the flux loads read (P22, P30).
    pub(crate) fn flux_profile(self) -> Option<&'static DriveProfile> {
        self.spec().flux_path
    }
}

impl HardDrive {
    fn spec(self) -> &'static HardDriveSpec {
        match self {
            Self::MbrSector => &MBR_SECTOR_HD_SPEC,
            Self::MbrBlock => &MBR_BLOCK_HD_SPEC,
            Self::Gpt => &GPT_HD_SPEC,
        }
    }

    /// The stable spelling, on the class enum too: a format field typed
    /// by the class ([`crate::Format::Qcow2`]'s `device`) is spelled in
    /// refusals before a whole [`DeviceType`] exists to ask.
    pub const fn id(self) -> &'static str {
        match self {
            Self::MbrSector => "mbr-sector-hd",
            Self::MbrBlock => "mbr-block-hd",
            Self::Gpt => "gpt-hd",
        }
    }
}

impl DeviceType {
    /// Every device type this release claims, class by class.
    pub const ALL: [Self; 7] = [
        Self::Floppy(FloppyDrive::Commodore1541),
        Self::Floppy(FloppyDrive::HeathH17),
        Self::Floppy(FloppyDrive::HeathH37),
        Self::Floppy(FloppyDrive::Sector),
        Self::HardDrive(HardDrive::MbrSector),
        Self::HardDrive(HardDrive::MbrBlock),
        Self::HardDrive(HardDrive::Gpt),
    ];

    /// The stable cross-language spelling — what survives in provenance,
    /// refusals, and the C and Python surfaces.
    pub fn id(self) -> &'static str {
        match self {
            Self::Floppy(floppy) => floppy.spec().id,
            Self::HardDrive(drive) => drive.spec().id,
        }
    }

    /// The type's name, fit to show a user beside the medium or the slot.
    pub fn name(self) -> &'static str {
        match self {
            Self::Floppy(floppy) => floppy.spec().name,
            Self::HardDrive(drive) => drive.spec().name,
        }
    }

    /// Where this type's declaration came from. A spec asserts recording
    /// facts, so it carries its source as the article and
    /// drive-profile catalogs do.
    pub fn provenance(self) -> &'static str {
        match self {
            Self::Floppy(floppy) => floppy.spec().provenance,
            Self::HardDrive(drive) => drive.spec().provenance,
        }
    }

    /// The class this type belongs to, by its stable spelling —
    /// `floppy` or `hard-drive`, with `optical` and `tape` reserved for
    /// the coming families.
    pub fn class(self) -> &'static str {
        match self {
            Self::Floppy(_) => "floppy",
            Self::HardDrive(_) => "hard-drive",
        }
    }

    /// The article this type is served, by the article catalog's
    /// stable spelling (P14). The article's own facts stay in that
    /// catalog; the type composes it and restates nothing.
    pub fn article(self) -> &'static str {
        self.article_profile().id
    }

    pub(crate) fn article_profile(self) -> &'static MediaProfile {
        match self {
            Self::Floppy(floppy) => floppy.spec().article,
            Self::HardDrive(drive) => drive.spec().article,
        }
    }

    /// The family half of every attachment identity a device of this
    /// type answers to — `hdd` for `hdd0`.
    pub fn slot_prefix(self) -> &'static str {
        match self {
            Self::Floppy(floppy) => floppy.spec().slot_prefix,
            Self::HardDrive(drive) => drive.spec().slot_prefix,
        }
    }

    /// The drive profile this type claims as its recording path (P22),
    /// by its stable spelling, or `None` where it claims none.
    pub fn flux_path(self) -> Option<&'static str> {
        match self {
            Self::Floppy(floppy) => floppy.spec().flux_path.map(|profile| profile.id),
            Self::HardDrive(_) => None,
        }
    }

    /// The partition scheme this type's spec lays its content out under,
    /// by its stable spelling — `Some` for the hard-drive class, whose
    /// specs carry the scheme, and `None` for the schemeless types,
    /// which bear the direct partition.
    pub fn scheme(self) -> Option<&'static str> {
        match self {
            Self::Floppy(_) => None,
            Self::HardDrive(drive) => Some(drive.spec().scheme.name()),
        }
    }

    /// How this type addresses its recording — `sector` or `block`.
    ///
    /// Every type declares one: it is part of the addressing surface the
    /// granularity rule says a device type fixes. A sector-addressed
    /// type is one whose medium answers
    /// [`get_sector`](crate::Medium::get_sector) and
    /// [`put_sector`](crate::Medium::put_sector), in the coordinates the
    /// medium's own geometry establishes; a block-addressed one refuses
    /// them by name, having no cylinder or head to be told about.
    pub fn addressing(self) -> &'static str {
        self.addressing_kind().name()
    }

    pub(crate) fn addressing_kind(self) -> Addressing {
        match self {
            Self::Floppy(floppy) => floppy.spec().addressing,
            Self::HardDrive(drive) => drive.spec().addressing,
        }
    }

    /// The scheme this type's partition pool populates under, as the
    /// evidence vocabulary spells it — or the named refusal for a
    /// declared scheme this release cannot yet read.
    ///
    /// Unreachable through a load today: no adapter declares a `gpt-hd`
    /// pairing, so the declaration refuses before a pool exists to
    /// populate. Spelled anyway, because an unreachable `unwrap` is a
    /// promise where a refusal is an answer (P6).
    pub(crate) fn readable_scheme(self) -> Option<Result<PartitionScheme>> {
        match self {
            Self::Floppy(_) => None,
            Self::HardDrive(drive) => Some(match drive.spec().scheme {
                DeclaredScheme::Mbr => Ok(PartitionScheme::Mbr),
                DeclaredScheme::Gpt => Err(Error::unsupported(format!(
                    "{} lays its content out under gpt, which no adapter in \
                     this release reads",
                    self.id()
                ))),
            }),
        }
    }

    /// Every device type served `article`, derived by asking the specs
    /// themselves — a query over the declarations, not a second list.
    ///
    /// An empty answer means no device this release claims is served the
    /// article — an outcome, not a defect in the medium.
    pub(crate) fn accepting(article: &'static MediaProfile) -> Vec<Self> {
        Self::ALL
            .into_iter()
            .filter(|device| device.article_profile().id == article.id)
            .collect()
    }

    /// Resolves a device type by its stable spelling, refusing one this
    /// release does not claim by name (P3).
    ///
    /// Rust callers hold the enum, where an unclaimed type fails to
    /// compile; this is for the C and Python surfaces, where the
    /// identity arrives as its stable spelling.
    pub fn from_id(id: &str) -> Result<Self> {
        Self::ALL
            .into_iter()
            .find(|device| device.id() == id)
            .ok_or_else(|| {
                let claimed: Vec<&str> = Self::ALL.iter().map(|device| device.id()).collect();
                Error::unsupported(format!(
                    "no device type named '{id}' is claimed; this release \
                     claims {}",
                    claimed.join(", ")
                ))
            })
    }
}

impl fmt::Display for DeviceType {
    /// The stable spelling — what a refusal or provenance line carries.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.id())
    }
}

/// What a slot in a machine is typed by, and what a medium asks for: a
/// recording device of one concrete type, or the archive receiver.
///
/// The receiver is not in the device-type catalog and never will be: a
/// device type names the device a recording is assumed made by, and an
/// archive was recorded by no device (`device_type()` answers `None`).
/// What an archive is loaded into is a receiver rather than a machine's
/// hardware, and it is a device for the reason every other holder is —
/// a caller never holds a medium outside one, and D27 keeps `arc0`
/// visible in its machine's attachment namespace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeviceSlot {
    /// A recording device of one concrete type.
    Recorded(DeviceType),
    /// The archive receiver: an attachment with no mechanism at all,
    /// which is the whole of what it is.
    Archive,
}

impl DeviceSlot {
    /// The stable cross-language spelling — a device type's own, or
    /// `archive` for the receiver.
    pub fn id(self) -> &'static str {
        match self {
            Self::Recorded(device) => device.id(),
            Self::Archive => "archive",
        }
    }

    /// The name, fit to show a user beside the slot it fills.
    pub fn name(self) -> &'static str {
        match self {
            Self::Recorded(device) => device.name(),
            Self::Archive => "archive receiver",
        }
    }

    /// The family half of every attachment identity in it — `arc` for
    /// `arc0`.
    pub fn slot_prefix(self) -> &'static str {
        match self {
            Self::Recorded(device) => device.slot_prefix(),
            Self::Archive => ARCHIVE_SLOT_PREFIX,
        }
    }

    /// The recording device this slot is typed by, or `None` for the
    /// receiver — the same `Option` a medium answers its device type
    /// with, and the same meaning: no device recorded this.
    pub fn device_type(self) -> Option<DeviceType> {
        match self {
            Self::Recorded(device) => Some(device),
            Self::Archive => None,
        }
    }

    /// Every slot a machine may hold a device in: one per device type,
    /// and the receiver.
    pub fn claimed() -> Vec<Self> {
        DeviceType::ALL
            .into_iter()
            .map(Self::Recorded)
            .chain(std::iter::once(Self::Archive))
            .collect()
    }

    /// Resolves a slot by its stable spelling, refusing one this release
    /// does not claim by name (P3). For the C and Python surfaces, where
    /// the identity arrives as text.
    pub fn from_id(id: &str) -> Result<Self> {
        if id == "archive" {
            return Ok(Self::Archive);
        }
        DeviceType::from_id(id).map(Self::Recorded)
    }

    /// The claimed slot prefixes, for the refusal a malformed attachment
    /// identity makes.
    pub(crate) fn prefixes() -> Vec<&'static str> {
        let mut prefixes: Vec<&'static str> =
            Self::claimed().into_iter().map(Self::slot_prefix).collect();
        prefixes.sort_unstable();
        prefixes.dedup();
        prefixes
    }

    /// The claimed spelling of `prefix`, where some claimed device fills
    /// that bay — the borrow an attachment identity keeps, since the
    /// identity outlives the text it was parsed from.
    pub(crate) fn claimed_prefix(prefix: &str) -> Option<&'static str> {
        Self::claimed()
            .into_iter()
            .map(Self::slot_prefix)
            .find(|claimed| *claimed == prefix)
    }
}

/// The receiver's slot prefix, stated once.
const ARCHIVE_SLOT_PREFIX: &str = "arc";

impl From<DeviceType> for DeviceSlot {
    fn from(device: DeviceType) -> Self {
        Self::Recorded(device)
    }
}

impl From<FloppyDrive> for DeviceSlot {
    fn from(floppy: FloppyDrive) -> Self {
        Self::Recorded(DeviceType::Floppy(floppy))
    }
}

impl From<HardDrive> for DeviceSlot {
    fn from(drive: HardDrive) -> Self {
        Self::Recorded(DeviceType::HardDrive(drive))
    }
}

impl fmt::Display for DeviceSlot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.id())
    }
}

/// How a refusal names a medium's recording side: the device type where
/// one recorded it, and the honest absence where none did.
pub(crate) fn recorded_reading(device: Option<DeviceType>) -> String {
    match device {
        Some(device) => format!("a {} ({}) recording", device.name(), device.id()),
        None => "a recording of no device — an archive's, or an authored \
                 medium's"
            .to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_type_is_named_once_and_carries_its_provenance() {
        for device in DeviceType::ALL {
            assert!(!device.id().is_empty(), "an entry with no id names nothing");
            assert!(!device.name().is_empty(), "{} has no name", device.id());
            assert!(
                !device.provenance().is_empty(),
                "{} declares recording facts from nowhere",
                device.id()
            );
            assert_eq!(
                DeviceType::from_id(device.id()).expect("a claimed type resolves"),
                device
            );
            assert!(
                !device.slot_prefix().chars().any(|c| c.is_ascii_digit()),
                "{} carries a digit, and an attachment identity parses as \
                 prefix-then-index",
                device.slot_prefix()
            );
        }

        let mut ids: Vec<&str> = DeviceType::ALL.iter().map(|device| device.id()).collect();
        let count = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), count, "two entries share an id");
    }

    #[test]
    fn the_two_levels_are_the_class_and_the_concrete_type() {
        assert_eq!(
            DeviceType::Floppy(FloppyDrive::Commodore1541).class(),
            "floppy"
        );
        assert_eq!(DeviceType::HardDrive(HardDrive::Gpt).class(), "hard-drive");
        assert_eq!(DeviceType::Floppy(FloppyDrive::Commodore1541).id(), "c1541");
        assert_eq!(
            DeviceType::HardDrive(HardDrive::MbrBlock).id(),
            "mbr-block-hd"
        );
        assert_eq!(HardDrive::MbrSector.id(), "mbr-sector-hd");
    }

    #[test]
    fn a_spec_composes_its_article_and_restates_nothing() {
        // D19's three homes: the article's facts stay in the article
        // catalog, reached through the composition rather than copied.
        assert_eq!(
            DeviceType::Floppy(FloppyDrive::Commodore1541).article(),
            "flexible-5.25-soft"
        );
        assert_eq!(
            DeviceType::Floppy(FloppyDrive::HeathH17).article(),
            "flexible-5.25-hard-10"
        );
        assert_eq!(
            DeviceType::Floppy(FloppyDrive::HeathH37).article(),
            "flexible-5.25-soft",
            "the H-37 is served the same article a 1541 is; the recording \
             discipline is what differs, and it lives in the type"
        );
        for drive in [HardDrive::MbrSector, HardDrive::MbrBlock, HardDrive::Gpt] {
            assert_eq!(DeviceType::HardDrive(drive).article(), "logical-block-512");
        }
    }

    #[test]
    fn the_hard_drive_specs_carry_the_scheme_itself() {
        assert_eq!(
            DeviceType::HardDrive(HardDrive::MbrSector).scheme(),
            Some("mbr")
        );
        assert_eq!(
            DeviceType::HardDrive(HardDrive::MbrBlock).scheme(),
            Some("mbr")
        );
        assert_eq!(DeviceType::HardDrive(HardDrive::Gpt).scheme(), Some("gpt"));
        assert_eq!(
            DeviceType::Floppy(FloppyDrive::Sector).scheme(),
            None,
            "the schemeless types bear the direct partition"
        );

        assert_eq!(
            DeviceType::HardDrive(HardDrive::MbrSector).addressing(),
            "sector"
        );
        assert_eq!(
            DeviceType::HardDrive(HardDrive::Gpt).addressing(),
            "block",
            "GPT implies block addressing by its own definition"
        );
        for floppy in [
            FloppyDrive::Commodore1541,
            FloppyDrive::HeathH17,
            FloppyDrive::HeathH37,
            FloppyDrive::Sector,
        ] {
            assert_eq!(
                DeviceType::Floppy(floppy).addressing(),
                "sector",
                "a floppy drive steps to a track and reads records around it, \
                 whatever geometry the disk in it was recorded under"
            );
        }

        // The declared scheme and the readable one are different claims:
        // MBR reads, and GPT refuses by name rather than pretending.
        assert_eq!(
            DeviceType::HardDrive(HardDrive::MbrBlock)
                .readable_scheme()
                .expect("a hard drive declares a scheme")
                .expect("mbr reads"),
            PartitionScheme::Mbr
        );
        let error = DeviceType::HardDrive(HardDrive::Gpt)
            .readable_scheme()
            .expect("a hard drive declares a scheme")
            .expect_err("no adapter reads gpt");
        assert!(error.to_string().contains("gpt"), "{error}");
    }

    #[test]
    fn the_flux_path_is_the_types_declaration() {
        // P22: the type declares the recording path, the drive profile
        // below it declares the rules, and the two must agree about the
        // article — a type served one disk whose profile is declared
        // over another would be describing two drives.
        assert_eq!(
            DeviceType::Floppy(FloppyDrive::Commodore1541).flux_path(),
            Some("c1541")
        );
        assert_eq!(drive_profile::C1541.media.id, "flexible-5.25-soft");
        for fluxless in [
            DeviceType::Floppy(FloppyDrive::HeathH17),
            DeviceType::Floppy(FloppyDrive::HeathH37),
            DeviceType::Floppy(FloppyDrive::Sector),
            DeviceType::HardDrive(HardDrive::MbrBlock),
        ] {
            assert_eq!(fluxless.flux_path(), None, "{}", fluxless.id());
        }
    }

    #[test]
    fn accepting_is_a_query_over_the_specs() {
        let soft: Vec<&str> = DeviceType::accepting(&FLEXIBLE_5_25_SOFT)
            .iter()
            .map(|device| device.id())
            .collect();
        assert_eq!(soft, vec!["c1541", "h37", "sector-floppy"]);

        let hard: Vec<&str> = DeviceType::accepting(&FLEXIBLE_5_25_HARD_10)
            .iter()
            .map(|device| device.id())
            .collect();
        assert_eq!(hard, vec!["h17"]);

        assert_eq!(
            DeviceType::accepting(&crate::model::media_profile::VIRTUAL).len(),
            0,
            "no drive is served the virtual article: an archive is held \
             by nothing"
        );
    }

    #[test]
    fn the_receiver_is_a_slot_and_no_device_type() {
        // An archive was recorded by no device, so the receiver answers
        // the same `None` a medium does — and it is not in the catalog.
        assert_eq!(DeviceSlot::Archive.device_type(), None);
        assert_eq!(DeviceSlot::Archive.id(), "archive");
        assert_eq!(DeviceSlot::Archive.slot_prefix(), "arc");
        assert!(
            DeviceType::from_id("archive").is_err(),
            "the receiver is no device type"
        );
        assert_eq!(
            DeviceSlot::from_id("archive").expect("claimed"),
            DeviceSlot::Archive
        );
        assert_eq!(
            DeviceSlot::from_id("c1541").expect("claimed"),
            DeviceSlot::Recorded(DeviceType::Floppy(FloppyDrive::Commodore1541))
        );
        assert_eq!(
            DeviceSlot::claimed().len(),
            DeviceType::ALL.len() + 1,
            "one slot per device type, and the receiver"
        );
    }

    #[test]
    fn types_may_share_a_slot_because_a_slot_names_a_place() {
        // Three hard drives fill one kind of bay and two Heathkit
        // controllers one kind of drive: the slot is where the device
        // sits, and the recording is what the device does.
        assert_eq!(
            DeviceType::HardDrive(HardDrive::MbrSector).slot_prefix(),
            DeviceType::HardDrive(HardDrive::Gpt).slot_prefix()
        );
        assert_eq!(
            DeviceType::Floppy(FloppyDrive::HeathH17).slot_prefix(),
            DeviceType::Floppy(FloppyDrive::HeathH37).slot_prefix()
        );
        assert_ne!(
            DeviceType::Floppy(FloppyDrive::Commodore1541).slot_prefix(),
            DeviceType::Floppy(FloppyDrive::HeathH17).slot_prefix()
        );
        assert_eq!(DeviceSlot::claimed_prefix("hdd"), Some("hdd"));
        assert_eq!(DeviceSlot::claimed_prefix("cdrom"), None);
        assert!(DeviceSlot::prefixes().contains(&"arc"));
    }

    #[test]
    fn an_unclaimed_type_is_refused_by_name() {
        // P3: the catalog is an enumerated claim. An optical drive is
        // the obvious next class and naming one must refuse rather than
        // approximate it from a floppy declaration.
        let error = DeviceType::from_id("cdrom").expect_err("refused");
        let message = error.to_string();
        assert!(message.contains("cdrom"), "names what was asked: {message}");
        assert!(
            message.contains("c1541"),
            "names what is claimed: {message}"
        );
        assert!(
            message.contains("mbr-block-hd"),
            "names the whole catalog: {message}"
        );
    }
}
