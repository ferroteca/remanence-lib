// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The layered disk inspection report.
//!
//! One inspection answers what a stopped machine's disk image contains
//! while preserving the boundaries between the facts it discovers: the
//! image adapter supplies one addressed device whose active durable layer
//! is block (P13, P23); a partition-schema adapter may expose addressed
//! regions on that device (P16); volume composition may form volumes from
//! the whole device or from those regions (P17); and a filesystem adapter
//! may recognize a filesystem on each volume (P18).
//!
//! Each record carries the vocabulary of its own seam. This is not a
//! recursive `Layer { kind, children }` tree: relationships express
//! provenance without pretending that a region is a volume or that a
//! filesystem is a property every volume possesses. A failure at one seam
//! never erases a record another seam owns, and never renumbers the
//! records behind it.

use crate::error::Error;

/// The tag half of a structural key, so identities of different kinds
/// never compare equal even when they name the same ordinal.
const REGION_TAG: u64 = 1;
const VOLUME_TAG: u64 = 2;
const FILESYSTEM_TAG: u64 = 3;

/// The ordinal a whole-device volume takes. Region-backed volumes take
/// their region's declared number, which is 1-based, so 0 is free.
const WHOLE_DEVICE: u64 = 0;

const fn structural_key(tag: u64, ordinal: u64) -> u64 {
    (tag << 56) | ordinal
}

/// Opaque identity for a declared partition region (P21).
///
/// The value is a deterministic function of the layout's structure, so an
/// unchanged single-disk layout issues an equal identity on a later open
/// in a later process, and a region that has gone never leaves its
/// identity for another to inherit. It is not an index into any list, and
/// [`value`](Self::value) exists so a binding can carry the identity
/// across an ABI — never so a caller can parse or construct one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RegionId(u64);

/// Opaque identity for a composed volume (P21). Same rules as
/// [`RegionId`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct VolumeId(u64);

/// Opaque identity for a recognized filesystem (P21). Same rules as
/// [`RegionId`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FilesystemId(u64);

macro_rules! opaque_id {
    ($id:ident, $tag:expr) => {
        impl $id {
            pub(crate) const fn of(ordinal: u64) -> Self {
                Self(structural_key($tag, ordinal))
            }

            /// The identity as one opaque value, for a binding to carry
            /// across an ABI. Its bits are the library's own and carry no
            /// documented meaning: a caller neither derives them from a
            /// partition number, an offset, a label, or a position, nor
            /// reads any of those back out.
            pub const fn value(self) -> u64 {
                self.0
            }

            /// The ordinal this identity was built from, for the library's
            /// own resolution of a reported record.
            #[allow(dead_code, reason = "generated uniformly for every identity kind")]
            pub(crate) const fn ordinal(self) -> u64 {
                self.0 & 0x00ff_ffff_ffff_ffff
            }
        }
    };
}

opaque_id!(RegionId, REGION_TAG);
opaque_id!(VolumeId, VOLUME_TAG);
opaque_id!(FilesystemId, FILESYSTEM_TAG);

impl RegionId {
    /// The identity of the region a schema declared at `number`. The
    /// declared number is the schema's own, and a region carrying an issue
    /// keeps it, so a refusal never renumbers what follows.
    pub(crate) const fn declared(number: u32) -> Self {
        Self::of(number as u64)
    }
}

impl VolumeId {
    /// Rebuilds an identity from a value a binding carried across an ABI.
    ///
    /// This exists so the C and Python presentations can hand back what a
    /// report gave them, and for nothing else. It is not a way to name a
    /// volume the library did not report: a value that was never issued
    /// simply resolves to no volume, and the verb refuses.
    pub const fn from_value(value: u64) -> Self {
        Self(value)
    }

    /// The identity of the volume composed from the whole device.
    pub(crate) const fn whole_device() -> Self {
        Self::of(WHOLE_DEVICE)
    }

    /// The identity of the volume composed from one declared region.
    pub(crate) const fn on_region(region: RegionId) -> Self {
        Self::of(region.ordinal())
    }
}

impl FilesystemId {
    /// The identity of the recognition attempted on one volume.
    pub(crate) const fn on_volume(volume: VolumeId) -> Self {
        Self::of(volume.ordinal())
    }
}

/// What the device's leading structure turned out to be.
///
/// This is a judgement the library already makes in order to compose
/// anything at all, so the report states it rather than leaving a caller
/// to reconstruct it from lists that are each empty for more than one
/// reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiskContent {
    /// The device is all zero: a blank disk, which is an answer.
    Blank,
    /// A partition schema was recognized, whether or not any volume
    /// composed from it.
    Schema,
    /// No partition schema, and the whole device is one volume.
    DirectVolume,
    /// The device is not blank and no adapter claims it. This is a
    /// reported outcome rather than a refusal: a disk in no format the
    /// library knows is a fact about the disk, not a failure of the
    /// inspection. An image that cannot be *read* still fails.
    UnknownNonblank {
        /// Why no adapter claimed it (P4).
        evidence: String,
    },
}

impl DiskContent {
    /// The stable cross-language spelling of this outcome.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Blank => "blank",
            Self::Schema => "schema",
            Self::DirectVolume => "direct-volume",
            Self::UnknownNonblank { .. } => "unknown-nonblank",
        }
    }
}

/// The one addressed device the image adapter supplied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    /// The device identity assigned by this loaded composition (P21). It
    /// is scoped to the open, unlike the layout-derived identities of the
    /// records below.
    pub id: u64,
    /// The image format the artifact turned out to be.
    pub image_format: String,
    /// The article of the medium attached here, named from the article
    /// catalog (P14). It says what the medium *is* — the substrate a
    /// drive would accept — and nothing about what is recorded on it,
    /// which the records below this one answer.
    pub article: String,
    /// The device this medium's content was recorded by, by the device
    /// catalog's stable spelling, or `None` where no device recorded it.
    ///
    /// It is the other half of what the article says: the substrate is
    /// what the disk is, and this is what wrote it — which is why the
    /// scheme this medium's content is laid out under is the device's
    /// declaration and not the article's.
    pub device_type: Option<String>,
    /// The device's addressable length in bytes.
    pub length_bytes: u64,
    /// The layer the image is authoritative at (P13).
    pub authoritative_layer: String,
    /// The layer active for this composition (P23). Block, for every
    /// composition this feature reaches.
    pub active_layer: String,
}

/// A recognized partition schema on the device (P16).
///
/// At most one, and it agrees with [`DiskContent::Schema`]: a list would
/// admit a second the outcome could not name, and the extended chain is
/// expressed as regions rather than as a schema nested inside one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartitionSchemaInfo {
    /// The schema kind, in its stable cross-language spelling.
    pub kind: String,
    /// What recognized it (P4).
    pub evidence: Vec<String>,
    /// Structured problems with the schema itself, as against with one of
    /// its declared regions.
    pub issues: Vec<Error>,
}

/// The role a region's schema gives it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegionRole {
    /// A region declaring data, which volume composition may compose.
    Data,
    /// A structural region — an extended partition. It is reported, and
    /// it is not thereby a volume.
    Structure,
}

impl RegionRole {
    /// The stable cross-language spelling of this role.
    pub fn name(self) -> &'static str {
        match self {
            Self::Data => "data",
            Self::Structure => "structure",
        }
    }
}

/// One region a partition schema declares (P16).
///
/// Every declared region is reported, including one whose type the library
/// declines to read and one whose volume could not be composed. A region
/// carrying an issue keeps its identity and its place, so the regions
/// behind it never renumber.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegionInfo {
    /// This region's opaque identity.
    pub id: RegionId,
    /// Where the region sits in its schema's own vocabulary — the slot or
    /// chain position it was declared at.
    pub declared_number: u32,
    /// How the schema places this region, in the schema's own vocabulary:
    /// for MBR, `"primary"` for one of the four slots and `"logical"` for
    /// an entry on the extended chain.
    ///
    /// This is a different axis from [`role`](Self::role) and neither
    /// implies the other: the extended region is a primary slot whose
    /// role is structural, while every logical entry is data.
    pub declared_placement: String,
    /// Whether the schema declares this region as data or as structure.
    pub role: RegionRole,
    /// The type value exactly as the schema records it.
    pub declared_type: u8,
    /// What that value *declares*, in a sentence fit to quote in a refusal
    /// a user reads — present whether or not the type is inside this
    /// release's read claim, because the region a caller most needs
    /// explained is the one the library refuses to read. It describes the
    /// declaration, never the content: an unread `0x07` region is not
    /// thereby asserted to hold NTFS.
    pub declared_type_reading: String,
    /// Whether this release reads the declared type.
    pub claimed: bool,
    pub start_bytes: u64,
    pub length_bytes: u64,
    /// The structured refusal that keeps this region in the report when
    /// its type is outside the claim or its chain could not be followed.
    pub issue: Option<Error>,
}

/// Where a volume's storage came from (P17).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VolumeOrigin {
    /// The whole device, with no partition schema between.
    WholeDevice,
    /// One or more declared regions, named by identity rather than by
    /// position.
    Regions(Vec<RegionId>),
}

/// One volume actually composed from what was available (P17).
///
/// A volume exists because composition succeeded, and filesystem
/// recognition neither creates it nor can erase it: a volume whose
/// filesystem is unknown or refused stays here, with the refusal owned by
/// the filesystem seam.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VolumeInfo {
    /// This volume's opaque identity.
    pub id: VolumeId,
    /// What this volume was composed from.
    pub origin: VolumeOrigin,
    pub start_bytes: u64,
    pub length_bytes: u64,
    /// What composed it (P4).
    pub evidence: Vec<String>,
    /// Structured problems with the composition itself.
    pub issues: Vec<Error>,
}

/// The geometry a filesystem's own boot record states, where it states
/// one. These are filesystem-declared facts and they manufacture no
/// physical drive: the device stays block-active.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DeclaredGeometry {
    pub sectors_per_track: Option<u16>,
    pub heads: Option<u16>,
    /// Only where the derivation is exact — the stated track geometry
    /// divides the total sector count with no remainder. Never invented.
    pub cylinders: Option<u64>,
}

/// One source's own reading of a volume label, kept beside the answer as
/// evidence (P4): a caller that needs what a particular structure holds
/// has it without opening a sector, and no caller has to know which of
/// the sources it should have looked at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LabelReading {
    /// The source, in the recognizing filesystem's own vocabulary and its
    /// stable cross-language spelling.
    pub source: String,
    /// What that source holds, as stored and less the format's own
    /// fixed-width padding — or `None` where the format gives this volume
    /// no such field at all, which is a third state distinct from a field
    /// that is present and blank.
    pub stored: Option<String>,
}

/// A recognized volume's label, answered whole.
///
/// The answer is [`name`](Self::name): the label, or `None` for a volume
/// that has none. A format's own spelling of unlabeled is resolved here,
/// where the format is known, rather than by a string comparison in every
/// consumer that displays a drive. Nothing outside the readings below may
/// become a label — not a directory name, not the filesystem kind, not a
/// file inside the volume, and not the image's own filename — and an
/// unlabeled volume is reported unlabeled rather than given a
/// placeholder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VolumeLabel {
    /// The label, or `None` where the volume has none.
    pub name: Option<String>,
    /// Which source decided the answer. `None` only where the volume
    /// carries no such source at all: a source that exists and says
    /// unlabeled is named here beside a [`name`](Self::name) of `None`.
    pub answered_by: Option<String>,
    /// Every source read, in the order the recognizing filesystem's own
    /// policy consults them.
    pub readings: Vec<LabelReading>,
}

/// What filesystem recognition found on one volume (P18).
///
/// A record exists wherever recognition was *attempted*, so a refusal has
/// a home at the seam that owns it rather than being parked on the volume
/// or dropped. A refused attempt carries no [`kind`](Self::kind) and its
/// [`issues`](Self::issues) say why; the volume it was attempted on stands
/// either way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilesystemInfo {
    /// This filesystem's opaque identity.
    pub id: FilesystemId,
    /// The volume it was recognized on.
    pub volume: VolumeId,
    /// The filesystem kind in its stable cross-language spelling, or
    /// `None` where recognition was refused.
    pub kind: Option<String>,
    /// The label answer, where a filesystem was recognized. `None` here is
    /// the absence of a *filesystem*, never of a label: a recognized
    /// volume's answer is always present and states absence itself.
    pub label: Option<VolumeLabel>,
    /// The allocation unit size, where the filesystem states one.
    pub cluster_bytes: Option<u64>,
    /// The allocation unit count, where the filesystem states one.
    pub cluster_count: Option<u64>,
    /// Geometry the filesystem's own structures declare.
    pub declared_geometry: DeclaredGeometry,
    /// What recognized it (P4).
    pub evidence: Vec<String>,
    /// Structured problems with the recognition. A refusal here belongs to
    /// this seam and leaves the underlying volume standing.
    pub issues: Vec<Error>,
}

/// The complete layered inspection of one disk.
///
/// Ordering exists for stable presentation and never supplies identity:
/// every relationship is traversed by an opaque identity, never by a
/// position in one of these lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiskReport {
    pub device: DeviceInfo,
    /// What the device's leading structure turned out to be, stated rather
    /// than left to be inferred from which lists came back empty.
    pub content: DiskContent,
    /// The recognized partition schema, if the device carries one.
    pub partition_schema: Option<PartitionSchemaInfo>,
    /// Every region the schema declares, refused ones included.
    pub regions: Vec<RegionInfo>,
    /// Every volume actually composed.
    pub volumes: Vec<VolumeInfo>,
    /// Every filesystem actually recognized on a volume.
    pub filesystems: Vec<FilesystemInfo>,
}

impl DiskReport {
    /// The region this identity names, or `None` where the report holds
    /// none.
    pub fn region(&self, id: RegionId) -> Option<&RegionInfo> {
        self.regions.iter().find(|region| region.id == id)
    }

    /// The volume this identity names, or `None` where the report holds
    /// none.
    pub fn volume(&self, id: VolumeId) -> Option<&VolumeInfo> {
        self.volumes.iter().find(|volume| volume.id == id)
    }

    /// The filesystem recognized on `volume`, or `None` where none was.
    /// Absence is an answer: the volume still exists.
    pub fn filesystem_on(&self, volume: VolumeId) -> Option<&FilesystemInfo> {
        self.filesystems
            .iter()
            .find(|filesystem| filesystem.volume == volume)
    }

    /// How many volumes were composed, whatever was or was not recognized
    /// on them.
    pub fn composed_volume_count(&self) -> usize {
        self.volumes.len()
    }

    /// How many volumes carry a filesystem the host actually read. It is
    /// deliberately distinct from
    /// [`composed_volume_count`](Self::composed_volume_count): an
    /// unrecognized volume stays in the report rather than vanishing to
    /// keep one number correct. It is a count and not a drive-letter
    /// rule — which volume a guest's letter named is
    /// [`DosMachine`](crate::DosMachine)'s answer, over a rule this
    /// library owns.
    pub fn readable_filesystem_volume_count(&self) -> usize {
        self.filesystems
            .iter()
            .filter(|filesystem| filesystem.kind.is_some() && filesystem.issues.is_empty())
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identities_of_different_kinds_never_collide_on_one_ordinal() {
        assert_ne!(RegionId::of(1).value(), VolumeId::of(1).value());
        assert_ne!(VolumeId::of(1).value(), FilesystemId::of(1).value());
        assert_ne!(RegionId::of(1).value(), FilesystemId::of(1).value());
    }

    #[test]
    fn an_identity_is_a_function_of_structure_not_of_discovery_order() {
        // The same declared number yields the same identity however many
        // times, and in whatever order, the layout is walked.
        assert_eq!(RegionId::of(3), RegionId::of(3));
        assert_ne!(RegionId::of(3), RegionId::of(4));
        assert_eq!(VolumeId::of(WHOLE_DEVICE), VolumeId::of(WHOLE_DEVICE));
        assert_ne!(VolumeId::of(WHOLE_DEVICE), VolumeId::of(1));
    }

    #[test]
    fn an_ordinal_round_trips_through_its_identity() {
        assert_eq!(RegionId::of(7).ordinal(), 7);
        assert_eq!(VolumeId::of(WHOLE_DEVICE).ordinal(), WHOLE_DEVICE);
        assert_eq!(FilesystemId::of(12).ordinal(), 12);
    }
}

#[cfg(test)]
mod stability {
    use super::*;

    /// Cross-open stability is only as strong as the derivation being a
    /// pure function of the layout, so the values are pinned outright. A
    /// counter, an index, or an address would satisfy opacity and fail
    /// this the first time a caller stored one.
    #[test]
    fn identity_values_are_pinned_functions_of_structure() {
        assert_eq!(RegionId::declared(1).value(), 0x0100_0000_0000_0001);
        assert_eq!(RegionId::declared(3).value(), 0x0100_0000_0000_0003);
        assert_eq!(VolumeId::whole_device().value(), 0x0200_0000_0000_0000);
        assert_eq!(
            VolumeId::on_region(RegionId::declared(3)).value(),
            0x0200_0000_0000_0003
        );
        assert_eq!(
            FilesystemId::on_volume(VolumeId::on_region(RegionId::declared(3))).value(),
            0x0300_0000_0000_0003
        );
    }
}
