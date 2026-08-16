// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The partition pool and the vantage doors (P16, P17, P19).
//!
//! **Partitions are the medium's evidence pool.** A medium's content is
//! reached through the partition that composes it: [`Medium::partition`]
//! by the scheme's own ordinal, [`Medium::partitions`] for the whole
//! pool, and [`Medium::partition_scheme`] for the scheme it was
//! populated under. Nothing here is discovered on demand — the pool is
//! established when the medium is loaded and is evidence from then on,
//! immutable for the session's life.
//!
//! **The pool populates under the medium's own kind, never by probing
//! for a layout.** A medium whose family is laid out under a scheme has
//! that scheme checked against its table at the load; a medium whose
//! family records no scheme bears the **direct partition** instead —
//! the library's own composition of the whole content, ordinal 0,
//! carried as provenance and never as evidence, and extent-less over a
//! medium whose native vantage is a namespace.
//!
//! **The vantage doors are pure lookups.** [`PartitionView::volume`]
//! answers where the partition composes an addressable extent and
//! [`PartitionView::filesystem`] where its declared type determines a
//! namespace; both hand out the one [`StorageSpace`] the partition
//! composes, which is D26's one node with two vantage traits reached
//! through two doors. Where nothing determines a namespace,
//! [`PartitionView::filesystem_as`] is the declared reading, and P18's
//! recognizer verifies that declaration rather than probing for one.
//!
//! [`mbr`] is the one scheme reader beneath this seam.

pub(crate) mod mbr;

use crate::error::{Error, ErrorCategory, Result, RuleIdentity};
use crate::filesystem::cbm_dos::BlockSource;
use crate::filesystem::{SpaceExtent, SpaceNamespace, StorageSpace};
use crate::model::media::Medium;
use crate::model::report::{DiskContent, RegionId, RegionRole, VolumeId};
use crate::partition::mbr::PartitionKind;

/// The ordinal the direct partition answers to.
///
/// A scheme numbers its own entries from one, so zero is the library's
/// to spend — and spending it here keeps the walk uniform: every medium
/// has a partition to be reached through, whether or not anything
/// recorded a table.
pub(crate) const DIRECT_ORDINAL: u32 = 0;

/// The direct partition's account of itself over a medium that was
/// loaded and records no scheme.
const RECORDS_NO_SCHEME: &str = "the library's own composition of the whole content: this \
                                 medium records no partition scheme, so the direct partition \
                                 is what the content is reached through — synthetic, and never \
                                 evidence";

/// One partition-layout scheme this release reads (P16).
///
/// The set is an enumerated claim: a medium's family names the scheme
/// its content is laid out under, and the scheme's own adapter owns the
/// recognition, the evidence and the refusals beneath that name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PartitionScheme {
    /// The MBR partition table: four primary slots and the extended
    /// chain, types pinned value by value.
    Mbr,
}

impl PartitionScheme {
    /// Every scheme this release reads.
    pub const ALL: [Self; 1] = [Self::Mbr];

    /// The stable cross-language spelling, which is what the C and Python
    /// surfaces carry.
    pub const fn id(self) -> &'static str {
        match self {
            Self::Mbr => "mbr",
        }
    }

    /// The scheme's name, fit to show a user.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Mbr => "MBR partition table",
        }
    }

    /// Reads a scheme back from its stable spelling, refusing one this
    /// release does not read naming what it does (P3).
    pub fn from_id(id: &str) -> Result<Self> {
        Self::ALL
            .into_iter()
            .find(|scheme| scheme.id() == id)
            .ok_or_else(|| {
                let claimed: Vec<&str> = Self::ALL.iter().map(|scheme| scheme.id()).collect();
                Error::unsupported(format!(
                    "'{id}' names no partition scheme this release reads; it reads {}",
                    claimed.join(", ")
                ))
            })
    }
}

impl std::fmt::Display for PartitionScheme {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.id())
    }
}

/// A declared reading of a partition's raw type value (P3).
///
/// It is the caller's reading and the library's check: a declaration
/// names one enumerated entry, and the entry's own type values are what
/// [`PartitionView::check_type`] weighs the recorded byte against. A
/// classification could check nothing and none is admitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PartitionType {
    /// A DOS data partition — the FAT type values this release reads
    /// (`0x01`, `0x04`, `0x06`, `0x0e`). It determines FAT, so a
    /// partition bearing it opens its namespace through the plain door.
    DosPrimary,
    /// The DOS extended container (`0x05`, `0x0f`). It is structure
    /// rather than data: it composes no volume, and it determines no
    /// namespace.
    DosExtended,
}

impl PartitionType {
    /// Every type a declaration may name.
    pub const ALL: [Self; 2] = [Self::DosPrimary, Self::DosExtended];

    /// The stable cross-language spelling.
    pub const fn id(self) -> &'static str {
        match self {
            Self::DosPrimary => "dos-primary",
            Self::DosExtended => "dos-extended",
        }
    }

    /// The type's name, fit to show a user.
    pub const fn name(self) -> &'static str {
        match self {
            Self::DosPrimary => "a DOS data partition",
            Self::DosExtended => "a DOS extended partition",
        }
    }

    /// Reads a type back from its stable spelling, for the C and Python
    /// surfaces where a declaration arrives as text (P3).
    pub fn from_id(id: &str) -> Result<Self> {
        Self::ALL
            .into_iter()
            .find(|declared| declared.id() == id)
            .ok_or_else(|| {
                let claimed: Vec<&str> = Self::ALL.iter().map(|declared| declared.id()).collect();
                Error::unsupported(format!(
                    "'{id}' names no partition type this release declares; it \
                 declares {}",
                    claimed.join(", ")
                ))
            })
    }

    /// The type values this reading covers, which is what a declaration
    /// is checked against.
    pub const fn declared_values(self) -> &'static [u8] {
        match self {
            Self::DosPrimary => &[0x01, 0x04, 0x06, 0x0e],
            Self::DosExtended => &[0x05, 0x0f],
        }
    }

    fn bears(self, type_byte: u8) -> bool {
        let mut at = 0;
        let values = self.declared_values();
        while at < values.len() {
            if values[at] == type_byte {
                return true;
            }
            at += 1;
        }
        false
    }
}

impl std::fmt::Display for PartitionType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.id())
    }
}

/// Which rule of the partition seam's enumerated set a refusal broke
/// (P10).
///
/// It answers *which rule did this input break* where the category
/// answers *how should the caller behave*, and it belongs to this seam
/// rather than to a second library-wide enum — [`crate::SpaceRule`] is
/// the same shape one vantage further in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartitionRule {
    /// A declared type the recorded value does not bear. The refusal
    /// names both sides: the reading declared and the byte recorded.
    TypeDisagrees,
    /// A declaration made of a partition that records no type at all.
    /// The direct partition is the library's own composition, so it has
    /// no recorded byte for a declaration to be checked against.
    NoDeclaredType,
    /// A namespace declared by a spelling this release does not read.
    UnclaimedNamespace,
    /// A namespace declared over a partition that composes no extent to
    /// read one out of.
    NoExtent,
}

impl PartitionRule {
    /// Every rule in the set.
    pub const ALL: [Self; 4] = [
        Self::TypeDisagrees,
        Self::NoDeclaredType,
        Self::UnclaimedNamespace,
        Self::NoExtent,
    ];

    /// The stable cross-language spelling of this rule, which is what a
    /// refusal carries.
    pub const fn as_str(self) -> RuleIdentity {
        match self {
            Self::TypeDisagrees => "partition-type-disagrees",
            Self::NoDeclaredType => "no-declared-type",
            Self::UnclaimedNamespace => "unclaimed-namespace",
            Self::NoExtent => "partition-no-extent",
        }
    }

    /// Reads a rule identity back into this set, for a caller branching
    /// on [`Error::rule`]. An identity from another seam's set is `None`
    /// rather than a nearest match.
    pub fn from_identity(identity: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|rule| rule.as_str() == identity)
    }
}

impl std::fmt::Display for PartitionRule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

fn refuse(rule: PartitionRule, reason: impl Into<String>) -> Error {
    let category = match rule {
        PartitionRule::TypeDisagrees => ErrorCategory::InvalidImage,
        PartitionRule::NoDeclaredType
        | PartitionRule::UnclaimedNamespace
        | PartitionRule::NoExtent => ErrorCategory::Unsupported,
    };
    Error::categorized_io(category, reason).broke_rule(rule.as_str())
}

/// What determines a partition's namespace vantage, where anything does.
///
/// It is a *declaration*, settled when the pool was established, and it
/// is what makes [`PartitionView::filesystem`] a lookup rather than a
/// probe. The recognizer runs to verify it and to fill values under it;
/// it never picks it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DeclaredNamespace {
    /// The declared partition type determines FAT, which the volume seam
    /// recognizes on the partition's own extent (P18).
    Fat,
    /// The medium's content *is* a namespace — the grammar that loaded
    /// the medium already read the names, so nothing is recognized here.
    Native,
}

/// The stable spellings a namespace declaration may name.
///
/// FAT is reached through the partition type that determines it, so a
/// declaration is only ever needed where nothing does: an HDOS or CP/M
/// catalog a medium bears directly, the CBM DOS directory a recording
/// bears, and FAT itself where no scheme declared a type.
/// The namespace names a caller may declare.
///
/// `cpm` is here as the name that *recognizes* a CP/M directory; it
/// refuses at the open, because the layout that says where a CP/M
/// volume's files are lived in the BIOS and is not on the disk. The
/// `cpm-*` entries beside it are the enrolled layouts, and each is the
/// narrowest claim its evidence supports — a release on a medium, not
/// CP/M in general.
const CLAIMED_NAMESPACES: [&str; 6] = [
    "fat",
    "hdos",
    "cpm",
    "cpm-heath-h17",
    "cpm-heath-soft",
    "cbmdos",
];

fn unclaimed_namespace(id: &str) -> Error {
    refuse(
        PartitionRule::UnclaimedNamespace,
        format!(
            "'{id}' names no namespace this release reads; it reads {}",
            CLAIMED_NAMESPACES.join(", ")
        ),
    )
}

/// One partition of a medium's evidence pool.
///
/// It carries what the scheme declared — the ordinal it was declared at,
/// its raw type value beside a reading of what that value declares, the
/// boot flag, its placement and its role (U4) — and what the library
/// composed over it. **The record is a value, not a handle**: it holds
/// nothing and outlives nothing, and the doors are reached through
/// [`Medium::partition`], which is the borrow that holds a partition and
/// its medium at once.
///
/// The **direct partition** is the one member the library composes
/// rather than reads. It stands where the medium records no scheme, it
/// declares no type, and its account is [`provenance`](Self::provenance)
/// rather than [`evidence`](Self::evidence) — a composition act stated
/// as one, never offered as something the medium said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Partition {
    ordinal: u32,
    placement: &'static str,
    role: RegionRole,
    active: bool,
    type_byte: Option<u8>,
    type_reading: Option<&'static str>,
    claimed: bool,
    start_bytes: Option<u64>,
    length_bytes: Option<u64>,
    addressable: bool,
    volume: Option<VolumeId>,
    namespace: Option<DeclaredNamespace>,
    issue: Option<Error>,
    evidence: Vec<String>,
    provenance: Option<&'static str>,
}

impl Partition {
    /// The scheme's own ordinal for this partition — MBR entry 1 is
    /// `1` — or `0` for the direct partition, which no scheme numbered.
    ///
    /// It is the scheme's evidence rather than a position in a list: a
    /// partition carrying an issue keeps its number, so the partitions
    /// behind it never renumber (U4).
    pub fn ordinal(&self) -> u32 {
        self.ordinal
    }

    /// Whether this is the direct partition — the library's own
    /// composition of the whole content, which stands where the medium
    /// records no scheme.
    pub fn is_direct(&self) -> bool {
        self.provenance.is_some()
    }

    /// Whether the scheme flags this partition active, as it records it.
    /// The direct partition is flagged by nothing and answers `false`.
    pub fn active(&self) -> bool {
        self.active
    }

    /// The type value exactly as the scheme records it, or `None` for
    /// the direct partition, which records none.
    pub fn type_byte(&self) -> Option<u8> {
        self.type_byte
    }

    /// What that value *declares*, in a sentence fit to quote in a
    /// refusal a user reads — present whether or not this release reads
    /// the type, because the partition a caller most needs explained is
    /// the one the library declines to read. It describes the
    /// declaration, never the content: an unread `0x07` partition is not
    /// thereby asserted to hold NTFS.
    pub fn type_reading(&self) -> Option<&str> {
        self.type_reading
    }

    /// Whether this release reads the declared type.
    pub fn is_claimed(&self) -> bool {
        self.claimed
    }

    /// How the scheme places this partition, in the scheme's own
    /// vocabulary: for MBR, `"primary"` for one of the four slots and
    /// `"logical"` for an entry on the extended chain. The direct
    /// partition answers `"direct"`.
    ///
    /// This is a different axis from [`role`](Self::role) and neither
    /// implies the other.
    pub fn placement(&self) -> &str {
        self.placement
    }

    /// Whether the scheme declares this partition as data or as
    /// structure.
    pub fn role(&self) -> RegionRole {
        self.role
    }

    /// Where this partition starts in the presented content, or `None`
    /// where it has no addressed extent at all — the direct partition
    /// over a medium whose native vantage is a namespace.
    pub fn start_bytes(&self) -> Option<u64> {
        self.start_bytes
    }

    /// How far this partition runs, under the same rule.
    pub fn length_bytes(&self) -> Option<u64> {
        self.length_bytes
    }

    /// Whether the addressable vantage opens — whether
    /// [`PartitionView::volume`] answers.
    pub fn is_addressable(&self) -> bool {
        self.addressable
    }

    /// Whether the namespace vantage opens — whether
    /// [`PartitionView::filesystem`] answers. Where it does not, the
    /// namespace is declared with [`PartitionView::filesystem_as`].
    pub fn bears_namespace(&self) -> bool {
        self.namespace.is_some()
    }

    /// The identity the inspection report issued for the volume composed
    /// over this partition, or `None` where it composed none. It is
    /// opaque and stable across opens of an unchanged layout (P21, U4).
    pub fn volume_id(&self) -> Option<VolumeId> {
        self.volume
    }

    /// The structured refusal that keeps this partition in the pool when
    /// its type is outside the claim or its chain could not be followed.
    /// A partition carrying one is still a partition, and still keeps its
    /// place.
    pub fn issue(&self) -> Option<&Error> {
        self.issue.as_ref()
    }

    /// What the scheme's adapter read to declare this partition (P4).
    /// The direct partition read nothing and states so through
    /// [`provenance`](Self::provenance) instead.
    pub fn evidence(&self) -> &[String] {
        &self.evidence
    }

    /// The direct partition's account of itself: what the library
    /// composed and why. It is present for the direct partition and
    /// absent for every partition a scheme declared, which is the whole
    /// of the distinction — a composition act is provenance, never
    /// evidence.
    pub fn provenance(&self) -> Option<&str> {
        self.provenance
    }

    /// The caller's own reading of the type, checked against the value
    /// the scheme recorded.
    ///
    /// The declaration is the caller's and the check is the library's: a
    /// reading the recorded byte does not bear is refused naming both
    /// sides, and the direct partition — which records no type — refuses
    /// by name rather than accepting a reading of nothing.
    pub fn check_type(&self, declared: PartitionType) -> Result<()> {
        let Some(type_byte) = self.type_byte else {
            return Err(refuse(
                PartitionRule::NoDeclaredType,
                format!(
                    "partition {} is the direct partition — the library's own \
                     composition of the whole content — and records no type \
                     for '{declared}' to be checked against",
                    self.ordinal
                ),
            ));
        };
        if declared.bears(type_byte) {
            return Ok(());
        }
        Err(refuse(
            PartitionRule::TypeDisagrees,
            format!(
                "partition {} records type 0x{type_byte:02x}, which declares \
                 {}; '{declared}' names {} and is borne by {}",
                self.ordinal,
                self.type_reading
                    .unwrap_or("no type this release has a reading for"),
                declared.name(),
                declared
                    .declared_values()
                    .iter()
                    .map(|value| format!("0x{value:02x}"))
                    .collect::<Vec<String>>()
                    .join(", ")
            ),
        ))
    }

    fn extent(&self) -> Option<SpaceExtent> {
        let (start_bytes, length_bytes) = (self.start_bytes?, self.length_bytes?);
        self.addressable.then_some(SpaceExtent {
            start_bytes,
            length_bytes,
            volume: self.volume,
        })
    }

    /// The direct partition over a medium's whole presented content.
    fn direct_over_space(
        length_bytes: u64,
        volume: Option<VolumeId>,
        provenance: &'static str,
    ) -> Self {
        Self {
            ordinal: DIRECT_ORDINAL,
            placement: "direct",
            role: RegionRole::Data,
            active: false,
            type_byte: None,
            type_reading: None,
            claimed: false,
            start_bytes: Some(0),
            length_bytes: Some(length_bytes),
            addressable: true,
            volume,
            namespace: None,
            issue: None,
            evidence: Vec::new(),
            provenance: Some(provenance),
        }
    }

    /// The direct partition over content whose native vantage is a
    /// namespace, which has no addressed extent to be a position within.
    fn direct_over_namespace(
        namespace: Option<DeclaredNamespace>,
        provenance: &'static str,
    ) -> Self {
        Self {
            ordinal: DIRECT_ORDINAL,
            placement: "direct",
            role: RegionRole::Data,
            active: false,
            type_byte: None,
            type_reading: None,
            claimed: false,
            start_bytes: None,
            length_bytes: None,
            addressable: false,
            volume: None,
            namespace,
            issue: None,
            evidence: Vec::new(),
            provenance: Some(provenance),
        }
    }
}

/// The borrow that holds a partition and its medium at once — the
/// partition's facts, and the two vantage doors onto the one
/// [`StorageSpace`] it composes.
///
/// **Both doors hand out the same node** (D26). `volume` answers where
/// the addressable vantage exists and `filesystem` where the namespace
/// vantage does; the space either door hands over carries whichever
/// vantages the partition has, so the choice is which question is being
/// asked rather than which object comes back. Opening a door spends the
/// view, which is that identity rule carried by the type.
#[derive(Debug)]
pub struct PartitionView<'a> {
    partition: Partition,
    backing: Backing<'a>,
}

/// What a partition's space is composed over.
enum Backing<'a> {
    /// A pooled medium: the space reads through it, by position and by
    /// name alike.
    Medium(&'a mut Medium),
    /// A recording's own record layer, reached through its own types
    /// rather than through a medium (P13). There is no medium and no
    /// addressed extent — only blocks the recording states addresses
    /// for.
    Blocks(&'a dyn BlockSource),
}

impl std::fmt::Debug for Backing<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Medium(_) => f.write_str("Medium"),
            Self::Blocks(_) => f.write_str("Blocks"),
        }
    }
}

impl<'a> PartitionView<'a> {
    pub(crate) fn over_medium(partition: Partition, medium: &'a mut Medium) -> Self {
        Self {
            partition,
            backing: Backing::Medium(medium),
        }
    }

    /// The direct partition over a recording's record layer — the one
    /// composition a layer no medium composed can bear.
    pub(crate) fn over_blocks(blocks: &'a dyn BlockSource) -> Self {
        Self {
            partition: Partition::direct_over_namespace(
                None,
                "the library's own composition of the whole recording: a \
                 recording records no partition scheme, so the direct \
                 partition is what its content is reached through — \
                 synthetic, and never evidence",
            ),
            backing: Backing::Blocks(blocks),
        }
    }

    /// This partition's record: everything the scheme declared about it,
    /// and everything the library composed over it.
    pub fn partition(&self) -> &Partition {
        &self.partition
    }

    /// The scheme's own ordinal for this partition.
    pub fn ordinal(&self) -> u32 {
        self.partition.ordinal()
    }

    /// Whether this is the direct partition.
    pub fn is_direct(&self) -> bool {
        self.partition.is_direct()
    }

    /// Whether the scheme flags this partition active.
    pub fn active(&self) -> bool {
        self.partition.active()
    }

    /// The type value exactly as the scheme records it.
    pub fn type_byte(&self) -> Option<u8> {
        self.partition.type_byte()
    }

    /// What that value declares.
    pub fn type_reading(&self) -> Option<&str> {
        self.partition.type_reading()
    }

    /// The caller's own reading of the type, checked against the value
    /// the scheme recorded — see [`Partition::check_type`].
    pub fn check_type(&self, declared: PartitionType) -> Result<()> {
        self.partition.check_type(declared)
    }

    /// The addressable vantage: the space this partition composes, read
    /// and written **by position within the partition's own extent**.
    ///
    /// It answers `None` where the partition composes no extent — a
    /// structural region, a type this release will not read, and the
    /// direct partition over a medium whose content is a namespace. The
    /// answer is a lookup: the extent was settled when the pool was
    /// established, and nothing is probed for here.
    pub fn volume(self) -> Option<StorageSpace<'a>> {
        if !self.partition.is_addressable() {
            return None;
        }
        Some(self.compose())
    }

    /// The namespace vantage: the same space, reached by the names it
    /// holds.
    ///
    /// It answers `None` where nothing determines a namespace over this
    /// partition — where the declared type determines none, or where no
    /// type is declared at all. That is the honest absence P19 requires
    /// rather than a failure to read one, and
    /// [`filesystem_as`](Self::filesystem_as) is where a caller who
    /// knows says so.
    pub fn filesystem(self) -> Option<StorageSpace<'a>> {
        if !self.partition.bears_namespace() {
            return None;
        }
        Some(self.compose())
    }

    /// The declared reading, where no partition type determines one:
    /// `"fat"`, `"hdos"`, `"cpm"`, a `"cpm-*"` layout, or `"cbmdos"`.
    ///
    /// **The reading is the caller's and the check is the library's.**
    /// The adapter the declaration names is the one that reads it, and
    /// it reads the evidence to verify the declaration rather than to
    /// pick one — a declaration the content cannot bear is refused by
    /// that adapter, by name. A spelling this release does not read is
    /// refused naming what it does (P3).
    pub fn filesystem_as(self, id: &str) -> Result<StorageSpace<'a>> {
        if !CLAIMED_NAMESPACES.contains(&id) {
            return Err(unclaimed_namespace(id));
        }
        let extent = self.partition.extent();
        match self.backing {
            Backing::Blocks(blocks) => {
                if id != crate::filesystem::cbm_dos::CBM_DOS {
                    return Err(refuse(
                        PartitionRule::UnclaimedNamespace,
                        format!(
                            "this partition is composed over a recording's own \
                             record layer, whose blocks are addressed by the \
                             recording rather than by position: the one \
                             namespace declared over it is \
                             '{}', not '{id}'",
                            crate::filesystem::cbm_dos::CBM_DOS
                        ),
                    ));
                }
                let catalog = crate::filesystem::cbm_dos::CbmDosCatalog::open(blocks)?;
                Ok(StorageSpace::over_catalog(
                    crate::filesystem::cbm_dos::CBM_DOS,
                    Box::new(catalog),
                ))
            }
            Backing::Medium(medium) => {
                if id == crate::filesystem::cbm_dos::CBM_DOS {
                    // CBM DOS is read off the record layer of a
                    // recording, so the declaration opens the flux
                    // medium's presentation ladder — materialized under
                    // the profile's declared policies — and refuses by
                    // name on a medium whose family bears no flux
                    // (P13, P30 reached through the type).
                    let sectors = medium.c1541_sectors()?;
                    let catalog = crate::filesystem::cbm_dos::CbmDosCatalog::open(sectors)?;
                    return Ok(StorageSpace::over_catalog(
                        crate::filesystem::cbm_dos::CBM_DOS,
                        Box::new(catalog),
                    ));
                }
                let Some(extent) = extent else {
                    return Err(refuse(
                        PartitionRule::NoExtent,
                        format!(
                            "this partition composes no addressed extent, so \
                             there is nothing for the '{id}' adapter to read a \
                             namespace out of"
                        ),
                    ));
                };
                let namespace = if id == "fat" {
                    let recognition = medium.recognize_fat(extent.start_bytes)?;
                    fat_namespace(extent.start_bytes, recognition)
                } else {
                    let adapter = crate::filesystem::catalog::by_id(id)?;
                    let catalog = medium.open_namespace_at(
                        adapter,
                        extent.start_bytes,
                        extent.length_bytes,
                    )?;
                    SpaceNamespace::Catalog {
                        kind: adapter.id(),
                        catalog,
                    }
                };
                Ok(StorageSpace::compose(medium, Some(extent), namespace))
            }
        }
    }

    /// The one space this partition composes, whichever door was opened.
    fn compose(self) -> StorageSpace<'a> {
        let extent = self.partition.extent();
        let declared = self.partition.namespace;
        match self.backing {
            Backing::Blocks(_) => unreachable!(
                "a partition over a record layer declares no namespace, so \
                 neither door opens without one being declared"
            ),
            Backing::Medium(medium) => {
                let namespace = match declared {
                    None => SpaceNamespace::Absent(refuse(
                        PartitionRule::UnclaimedNamespace,
                        "nothing declares a namespace over this partition; \
                         declare one with `filesystem_as`"
                            .to_owned(),
                    )),
                    Some(DeclaredNamespace::Native) => match medium.archive_namespace() {
                        Some((kind, catalog)) => SpaceNamespace::Catalog { kind, catalog },
                        None => SpaceNamespace::Absent(refuse(
                            PartitionRule::UnclaimedNamespace,
                            "this medium's content was declared a namespace and \
                             holds none"
                                .to_owned(),
                        )),
                    },
                    Some(DeclaredNamespace::Fat) => {
                        // The declaration is the partition type's; this is the
                        // verification under it, and a refusal here belongs to
                        // the seam that made it (P4, P10).
                        let offset = extent.map_or(0, |extent| extent.start_bytes);
                        match medium.recognize_fat(offset) {
                            Ok(recognition) => fat_namespace(offset, recognition),
                            Err(absence) => SpaceNamespace::Absent(absence),
                        }
                    }
                };
                StorageSpace::compose(medium, extent, namespace)
            }
        }
    }
}

fn fat_namespace(
    offset: u64,
    recognition: crate::filesystem::fat::FatRecognition,
) -> SpaceNamespace<'static> {
    SpaceNamespace::Fat {
        offset,
        kind: recognition.kind.name().to_owned(),
        label: recognition.label,
        evidence: vec!["a FAT boot record recognized at the partition's first sector".to_owned()],
    }
}

/// A medium's partition pool: the scheme it was populated under, and
/// every partition in it.
///
/// It is established once, when the medium is loaded, and is evidence
/// from then on. Nothing re-reads a table behind a caller's back, and
/// nothing adds a partition to a medium that was loaded without one —
/// the create and release slots a partition editor would need are
/// deliberately unfilled.
#[derive(Debug, Clone)]
pub(crate) struct PartitionPool {
    scheme: Option<PartitionScheme>,
    /// What the scheme's adapter read to recognize the table (P4), empty
    /// where no scheme was recorded.
    schema_evidence: Vec<String>,
    /// What the content's leading structure turned out to be, stated by
    /// the scheme's adapter when it checked the scheme and carried here
    /// so the inspection report derives it rather than re-reading for it.
    content: DiskContent,
    partitions: Vec<Partition>,
}

impl PartitionPool {
    pub(crate) fn scheme(&self) -> Option<PartitionScheme> {
        self.scheme
    }

    pub(crate) fn content(&self) -> DiskContent {
        self.content.clone()
    }

    pub(crate) fn schema_evidence(&self) -> &[String] {
        &self.schema_evidence
    }

    pub(crate) fn partitions(&self) -> &[Partition] {
        &self.partitions
    }

    pub(crate) fn get(&self, ordinal: u32) -> Option<&Partition> {
        self.partitions
            .iter()
            .find(|partition| partition.ordinal == ordinal)
    }

    /// The pool a medium whose content is its own namespace bears: one
    /// direct partition, extent-less, its namespace the grammar the load
    /// already read.
    pub(crate) fn native_namespace() -> Self {
        Self {
            scheme: None,
            schema_evidence: Vec::new(),
            content: DiskContent::DirectVolume,
            partitions: vec![Partition::direct_over_namespace(
                Some(DeclaredNamespace::Native),
                "the library's own composition of the whole content: this \
                 medium's native vantage is a namespace, so it records no \
                 partition scheme and has no addressed extent to be a \
                 position within — synthetic, and never evidence",
            )],
        }
    }

    /// The pool a flux medium bears: one direct partition, extent-less
    /// and namespace-less — a recording records no partition scheme,
    /// its blocks are addressed by the recording rather than by
    /// position, and the one namespace over it is a declared reading
    /// (`filesystem_as`), because nothing here determines one and this
    /// layer will not pick one.
    pub(crate) fn over_recording() -> Self {
        Self {
            scheme: None,
            schema_evidence: Vec::new(),
            content: DiskContent::DirectVolume,
            partitions: vec![Partition::direct_over_namespace(
                None,
                "the library's own composition of the whole recording: a flux \
                 medium records no partition scheme, so the direct partition \
                 is what its content is reached through — synthetic, and \
                 never evidence",
            )],
        }
    }

    /// The pool an authored medium bears whose kind states coordinates:
    /// the direct partition over the whole of the content the author
    /// created, blank by construction.
    ///
    /// Nothing is classified and nothing is read, because there is no
    /// artifact to read: a blank the author just made is blank, which is
    /// the one case where the content's own answer is known without
    /// asking it. The addressable vantage opens over it; no namespace
    /// does, because nothing recorded one and the arc that would is
    /// reserved.
    pub(crate) fn authored_space(length_bytes: u64) -> Self {
        Self::direct(
            DiskContent::Blank,
            length_bytes,
            None,
            "the library's own composition of the whole content: this medium \
             was authored and nothing has been recorded onto it, so it \
             records no partition scheme and the direct partition is what \
             its content is reached through — synthetic, and never evidence",
        )
    }

    /// The pool an authored **blank article** bears: one direct
    /// partition, extent-less and namespace-less. The article is the
    /// whole of what the author stated, and nothing is recorded on it to
    /// be a position within.
    pub(crate) fn authored_blank() -> Self {
        Self {
            scheme: None,
            schema_evidence: Vec::new(),
            content: DiskContent::Blank,
            partitions: vec![Partition::direct_over_namespace(
                None,
                "the library's own composition of the whole medium: this blank \
                 was authored as an article and nothing is recorded on it, so \
                 the direct partition has no addressed extent and no namespace \
                 — synthetic, and never evidence",
            )],
        }
    }

    /// The pool a medium bears whose **device type declares no scheme**:
    /// the direct partition over the whole content, with no table read
    /// and none looked for.
    ///
    /// The content is still classified — blank, one bare volume, or
    /// content nothing claims — because that is a fact about what was
    /// recorded rather than about a layout, and the direct partition
    /// composes a volume exactly where a volume is there to compose.
    pub(crate) fn over_schemeless(content: &mbr::Discovery, length_bytes: u64) -> Self {
        match content {
            mbr::Discovery::BareVolume => Self::direct(
                DiskContent::DirectVolume,
                length_bytes,
                Some(VolumeId::whole_device()),
                RECORDS_NO_SCHEME,
            ),
            mbr::Discovery::Blank => {
                Self::direct(DiskContent::Blank, length_bytes, None, RECORDS_NO_SCHEME)
            }
            mbr::Discovery::UnknownNonblank { evidence } => Self::direct(
                DiskContent::UnknownNonblank {
                    evidence: evidence.clone(),
                },
                length_bytes,
                None,
                RECORDS_NO_SCHEME,
            ),
            // Unreachable by construction: the schemeless classifier
            // never reads a table. Spelled as the same absence rather
            // than as a panic, because a medium whose type declares no
            // scheme has no partitioned answer to give.
            mbr::Discovery::Partitioned(_) => Self::direct(
                DiskContent::UnknownNonblank {
                    evidence: mbr::UNDECLARED_TABLE.to_owned(),
                },
                length_bytes,
                None,
                RECORDS_NO_SCHEME,
            ),
        }
    }

    /// The pool a medium whose content is a space bears: the scheme's own
    /// entries where its table checks out, and the direct partition where
    /// the medium records no scheme at all.
    pub(crate) fn over_space(
        scheme: PartitionScheme,
        discovery: &mbr::Discovery,
        length_bytes: u64,
    ) -> Self {
        match discovery {
            mbr::Discovery::Partitioned(rows) => Self {
                scheme: Some(scheme),
                schema_evidence: vec![
                    "sector 0 carries the boot signature and parses as an MBR \
                     partition table"
                        .to_owned(),
                ],
                content: DiskContent::Schema,
                partitions: rows.iter().map(declared_partition).collect(),
            },
            // A filesystem boot record, a blank disk, and content nothing
            // claims are three different answers about the same absence:
            // this medium records no scheme, and the direct partition is
            // what its content is reached through. They are the same
            // three answers a medium whose type declares no scheme gets,
            // which is why the reading is the one below.
            content => Self::over_schemeless(content, length_bytes),
        }
    }

    fn direct(
        content: DiskContent,
        length_bytes: u64,
        volume: Option<VolumeId>,
        provenance: &'static str,
    ) -> Self {
        Self {
            scheme: None,
            schema_evidence: Vec::new(),
            content,
            partitions: vec![Partition::direct_over_space(
                length_bytes,
                volume,
                provenance,
            )],
        }
    }
}

/// One partition as the scheme declared it, with what the library
/// composes over it settled in the same act.
fn declared_partition(row: &mbr::PartitionInfo) -> Partition {
    let region = RegionId::declared(row.number);
    let role = if mbr::is_extended(row.type_byte) {
        RegionRole::Structure
    } else {
        RegionRole::Data
    };
    let claimed = row.type_name.is_some();
    // A structural region is declared and is not thereby a volume; a type
    // this release will not read composes nothing; and a row carrying an
    // issue composes nothing either. All three keep their place.
    let addressable = role == RegionRole::Data && claimed && row.issue.is_none();
    Partition {
        ordinal: row.number,
        placement: row.kind.name(),
        role,
        active: row.active,
        type_byte: Some(row.type_byte),
        type_reading: Some(mbr::declared_type_reading(row.type_byte)),
        claimed,
        start_bytes: Some(row.start_bytes),
        length_bytes: Some(row.length_bytes),
        addressable,
        volume: addressable.then(|| VolumeId::on_region(region)),
        // The declared type is what determines the namespace vantage, and
        // the only type this release reads determines FAT.
        namespace: (addressable && PartitionType::DosPrimary.bears(row.type_byte))
            .then_some(DeclaredNamespace::Fat),
        issue: row.issue.clone(),
        evidence: vec![format!(
            "declared by the MBR partition table at {} entry {}",
            match row.kind {
                PartitionKind::Primary => "primary",
                PartitionKind::Logical => "extended-chain",
            },
            row.number
        )],
        provenance: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn declared(number: u32, type_byte: u8) -> Partition {
        declared_partition(&mbr::PartitionInfo {
            number,
            kind: PartitionKind::Primary,
            active: number == 1,
            type_byte,
            type_name: mbr::declared_type_reading(type_byte)
                .starts_with("FAT")
                .then(|| "FAT".to_owned()),
            start_bytes: 1_048_576,
            length_bytes: 4_096_000,
            issue: None,
        })
    }

    #[test]
    fn every_claimed_scheme_and_type_round_trips_through_its_spelling() {
        for scheme in PartitionScheme::ALL {
            assert_eq!(
                PartitionScheme::from_id(scheme.id()).expect("claimed"),
                scheme
            );
            assert!(!scheme.name().is_empty());
        }
        for declared in PartitionType::ALL {
            assert_eq!(
                PartitionType::from_id(declared.id()).expect("claimed"),
                declared
            );
            assert!(!declared.declared_values().is_empty());
        }
        for rule in PartitionRule::ALL {
            assert_eq!(PartitionRule::from_identity(rule.as_str()), Some(rule));
        }
        assert!(PartitionRule::from_identity("outside-extent").is_none());
    }

    #[test]
    fn a_declared_reading_is_checked_against_the_recorded_byte() {
        // The caller declares and the library checks: 0x06 bears a DOS
        // data partition and 0x05 does not, and the refusal names both
        // sides rather than saying only that something disagreed.
        let fat = declared(1, 0x06);
        assert!(fat.check_type(PartitionType::DosPrimary).is_ok());
        let error = fat
            .check_type(PartitionType::DosExtended)
            .expect_err("0x06 is no extended container");
        assert_eq!(error.rule(), Some(PartitionRule::TypeDisagrees.as_str()));
        let message = error.to_string();
        assert!(
            message.contains("0x06"),
            "names what was recorded: {message}"
        );
        assert!(
            message.contains("dos-extended"),
            "names what was declared: {message}"
        );

        let extended = declared(2, 0x05);
        assert!(extended.check_type(PartitionType::DosExtended).is_ok());
        assert!(extended.check_type(PartitionType::DosPrimary).is_err());
    }

    #[test]
    fn the_direct_partition_declares_no_type_and_says_so_by_name() {
        let direct = Partition::direct_over_space(
            4_096_000,
            Some(VolumeId::whole_device()),
            RECORDS_NO_SCHEME,
        );
        assert!(direct.is_direct());
        assert_eq!(direct.ordinal(), DIRECT_ORDINAL);
        assert_eq!(direct.type_byte(), None);
        assert!(
            direct.provenance().is_some(),
            "a composition act is provenance"
        );
        assert!(direct.evidence().is_empty(), "and never evidence");
        let error = direct
            .check_type(PartitionType::DosPrimary)
            .expect_err("there is no byte to check against");
        assert_eq!(error.rule(), Some(PartitionRule::NoDeclaredType.as_str()));
    }

    #[test]
    fn a_structural_region_composes_no_space_and_keeps_its_place() {
        let extended = declared(2, 0x05);
        assert_eq!(extended.role(), RegionRole::Structure);
        assert!(
            !extended.is_addressable(),
            "an extended container is not a volume"
        );
        assert!(!extended.bears_namespace());
        assert_eq!(extended.ordinal(), 2, "nothing renumbers");
        assert_eq!(
            extended.start_bytes(),
            Some(1_048_576),
            "it is still declared"
        );
    }

    #[test]
    fn an_unread_type_is_explained_and_composes_nothing() {
        let ntfs = declared(1, 0x07);
        assert!(!ntfs.is_claimed());
        assert_eq!(ntfs.type_reading(), Some("NTFS or exFAT"));
        assert!(!ntfs.is_addressable());
        assert!(!ntfs.bears_namespace());
        assert_eq!(ntfs.volume_id(), None);
    }

    #[test]
    fn the_boot_flag_is_read_as_the_scheme_records_it() {
        assert!(declared(1, 0x06).active(), "slot 1 carries the flag here");
        assert!(!declared(2, 0x06).active());
    }

    #[test]
    fn a_namespace_this_release_does_not_read_is_refused_by_name() {
        let error = unclaimed_namespace("ext4");
        assert_eq!(
            error.rule(),
            Some(PartitionRule::UnclaimedNamespace.as_str())
        );
        let message = error.to_string();
        assert!(message.contains("ext4"), "names what was asked: {message}");
        assert!(message.contains("hdos"), "names what is read: {message}");
    }
}
