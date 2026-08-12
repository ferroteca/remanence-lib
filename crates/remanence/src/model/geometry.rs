// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! Discovered geometry, and the recording's own coordinates.
//!
//! **Geometry is discovered instance evidence, never a declaration onto
//! a medium.** What a device fixes lives in its type (P14's granularity
//! rule); how one disk was laid out varies disk to disk, so it is read
//! off the artifact — and every reading says where it came from (P4).
//! The sources are enumerated ([`GeometrySource`]): the image format's
//! own declaration where it makes one, the load's declaration of a raw
//! image's block size, the FAT boot record's recorded sectors-per-track
//! and heads, the MBR table's end tuples, and extent arithmetic over the
//! content under the track geometry the others fixed.
//!
//! **[`Authorship`](GeometrySource::Authorship) is the one source that
//! reads nothing**, and it belongs to the one medium with no artifact
//! beneath it: a caller who creates media whole
//! ([`Session::new_media`](crate::Session::new_media)) states the
//! coordinates in that same act, and they are the medium's own facts
//! from then on. It is not an exception to the sentence above — nothing
//! is declared onto a medium that *exists* — and it never stands beside
//! another source, so nothing here has to settle authorship against
//! evidence.
//!
//! **Sources that disagree settle nothing.** Where two readings state
//! different values for one part of the coordinates, that part is
//! unsettled, the medium's geometry is
//! [`Undetermined`](GeometryState::Undetermined), and both readings come
//! back with a sentence naming what disagrees with what. Nothing here
//! ranks sources, breaks ties, or picks the likelier answer: a caller
//! who is going to act on a geometry is owed the fact that the artifact
//! does not state one, not the library's best guess at it.
//!
//! **Where nothing states one at all**, the answer is
//! [`Unstated`](GeometryState::Unstated) — a third state, kept distinct
//! from disagreement, because "no source spoke" and "the sources
//! contradict each other" are different facts about the artifact.
//!
//! The coordinates themselves are [`RecordingGeometry`], and
//! [`Medium::get_sector`](crate::Medium::get_sector) and
//! [`Medium::put_sector`](crate::Medium::put_sector) are what address in
//! them. Cylinders and heads number from zero and **sectors number from
//! one**, which is the recording's own convention rather than this
//! library's.

use crate::error::{Error, ErrorCategory, Result, RuleIdentity};
use crate::filesystem::fat;
use crate::image::adapters::DiskDescriptor;
use crate::io::device::Device;
use crate::partition::mbr;

/// One source that may state part of a recording's coordinates (P3).
///
/// The set is enumerated because a reading is only worth anything with
/// its origin attached: "16 heads" is a different fact when a boot
/// record wrote it than when a partition table's end tuple implies it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GeometrySource {
    /// The image format's own declaration — the geometry an adapter
    /// records for every image it claims, or the block size a raw
    /// reading's load declared.
    FormatDeclaration,
    /// A filesystem boot record's recorded geometry — the FAT BPB's
    /// sectors-per-track, heads and bytes-per-sector.
    BootRecord,
    /// The partition table: the addressing unit its own extents are
    /// numbered in, and the geometry its end tuples imply where one
    /// checks out against the extent it declares.
    PartitionTable,
    /// Arithmetic over the content's extent, under the track geometry
    /// the other sources fixed. It states the cylinder count and nothing
    /// else, because that is all an extent can say.
    ExtentArithmetic,
    /// The author's own statement, made when the medium was created
    /// ([`Session::new_media`](crate::Session::new_media)).
    ///
    /// It is the one source that is not a reading of an artifact, and it
    /// belongs to the one medium that has none: authorship is its own
    /// fact class, so an authored blank's coordinates are the author's
    /// facts rather than evidence about anything. It never appears
    /// beside another source — nothing is authored onto a medium that
    /// was loaded, and nothing is discovered onto one that was authored.
    Authorship,
}

impl GeometrySource {
    /// Every source this release reads a geometry out of.
    pub const ALL: [Self; 5] = [
        Self::FormatDeclaration,
        Self::BootRecord,
        Self::PartitionTable,
        Self::ExtentArithmetic,
        Self::Authorship,
    ];

    /// The stable cross-language spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::FormatDeclaration => "format-declaration",
            Self::BootRecord => "boot-record",
            Self::PartitionTable => "partition-table",
            Self::ExtentArithmetic => "extent-arithmetic",
            Self::Authorship => "authorship",
        }
    }

    /// Reads a source back from its stable spelling, for the C and
    /// Python surfaces. A spelling outside the set is `None` rather than
    /// a nearest match.
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|source| source.as_str() == id)
    }
}

impl std::fmt::Display for GeometrySource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What the evidence established about a medium's geometry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GeometryState {
    /// No source beneath this medium states a whole geometry. It is not
    /// a failure to read one: an archive has no coordinates at all, and
    /// a block image with no boot record and no table states nothing.
    Unstated,
    /// Every part of the coordinates is established, and the readings
    /// that established it agree.
    Determined,
    /// Two sources state different values for the same part of the
    /// coordinates. Both readings stand and neither settles it.
    Undetermined,
}

impl GeometryState {
    /// Every state a medium's geometry may be in.
    pub const ALL: [Self; 3] = [Self::Unstated, Self::Determined, Self::Undetermined];

    /// The stable cross-language spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unstated => "unstated",
            Self::Determined => "determined",
            Self::Undetermined => "undetermined",
        }
    }
}

impl std::fmt::Display for GeometryState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Which rule of the geometry seam's enumerated set a refusal broke
/// (P10).
///
/// It answers *which rule did this input break* where the category
/// answers *how should the caller behave*, and it belongs to this seam
/// rather than to a second library-wide enum — [`crate::PartitionRule`]
/// and [`crate::SpaceRule`] are the same shape at their own seams.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeometryRule {
    /// A sector coordinate over a recording addressed some other way —
    /// a block-addressed drive, or a medium no device recorded.
    NotSectorAddressed,
    /// A sector coordinate over a medium whose geometry no source
    /// states.
    GeometryUnstated,
    /// A sector coordinate over a medium whose sources disagree about
    /// its geometry.
    GeometryUndetermined,
    /// A coordinate outside the geometry the evidence established, or
    /// inside it and past the content the medium actually holds.
    OutsideGeometry,
    /// A buffer that is not one whole sector of this recording. A sector
    /// is answered whole or not at all.
    PartialSector,
}

impl GeometryRule {
    /// Every rule in the set.
    pub const ALL: [Self; 5] = [
        Self::NotSectorAddressed,
        Self::GeometryUnstated,
        Self::GeometryUndetermined,
        Self::OutsideGeometry,
        Self::PartialSector,
    ];

    /// The stable cross-language spelling of this rule, which is what a
    /// refusal carries.
    pub const fn as_str(self) -> RuleIdentity {
        match self {
            Self::NotSectorAddressed => "not-sector-addressed",
            Self::GeometryUnstated => "geometry-unstated",
            Self::GeometryUndetermined => "geometry-undetermined",
            Self::OutsideGeometry => "outside-geometry",
            Self::PartialSector => "partial-sector",
        }
    }

    /// Reads a rule identity back into this set, for a caller branching
    /// on [`Error::rule`]. An identity from another seam's set is `None`
    /// rather than a nearest match.
    pub fn from_identity(identity: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|rule| rule.as_str() == identity)
    }
}

impl std::fmt::Display for GeometryRule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

pub(crate) fn refuse(rule: GeometryRule, reason: impl Into<String>) -> Error {
    let category = match rule {
        GeometryRule::OutsideGeometry => ErrorCategory::NotFound,
        GeometryRule::NotSectorAddressed
        | GeometryRule::GeometryUnstated
        | GeometryRule::GeometryUndetermined
        | GeometryRule::PartialSector => ErrorCategory::Unsupported,
    };
    Error::categorized_io(category, reason).broke_rule(rule.as_str())
}

/// The recording's own coordinates: how a recorded disk divides into
/// cylinders, heads, sectors and bytes.
///
/// Cylinders and heads number from zero; **sectors number from one**.
/// Every part is present — a partial geometry addresses nothing, so it
/// is [`GeometryState::Unstated`] rather than a record with holes in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RecordingGeometry {
    /// How many cylinders the recording spans. Where the content is not
    /// a whole number of cylinders, this is the cylinder its last sector
    /// falls in, plus one — the extent's own reading says so, and a
    /// coordinate in the part the content does not hold is refused by
    /// name rather than answered with zeros.
    pub cylinders: u32,
    /// How many heads — recorded surfaces — the geometry addresses.
    pub heads: u32,
    /// How many sectors each track holds.
    pub sectors_per_track: u32,
    /// How many bytes one sector holds.
    pub sector_bytes: u64,
}

impl RecordingGeometry {
    /// How many sectors the whole geometry addresses.
    pub fn total_sectors(self) -> u64 {
        u64::from(self.cylinders) * u64::from(self.heads) * u64::from(self.sectors_per_track)
    }

    /// How many bytes the whole geometry addresses.
    pub fn total_bytes(self) -> u64 {
        self.total_sectors() * self.sector_bytes
    }

    /// The same, where coordinates that came from nowhere but a caller
    /// might not fit in one: `None` where the product overflows.
    ///
    /// Discovered coordinates are read off an artifact that exists, so
    /// they describe something a host already holds; authored ones are
    /// stated, and stating them is the moment to find out that no medium
    /// could hold what was asked for.
    pub(crate) fn checked_total_bytes(self) -> Option<u64> {
        u64::from(self.cylinders)
            .checked_mul(u64::from(self.heads))?
            .checked_mul(u64::from(self.sectors_per_track))?
            .checked_mul(self.sector_bytes)
    }

    /// Where one coordinate sits in the content, in CHS order, or the
    /// refusal naming the geometry it lies outside.
    pub(crate) fn offset_of(self, cylinder: u32, head: u32, sector: u32) -> Result<u64> {
        if cylinder >= self.cylinders
            || head >= self.heads
            || sector == 0
            || sector > self.sectors_per_track
        {
            return Err(refuse(
                GeometryRule::OutsideGeometry,
                format!(
                    "cylinder {cylinder}, head {head}, sector {sector} lies \
                     outside this recording's coordinates: {self}, with \
                     cylinders and heads numbered from 0 and sectors from 1"
                ),
            ));
        }
        let track = u64::from(cylinder) * u64::from(self.heads) + u64::from(head);
        let index = track * u64::from(self.sectors_per_track) + u64::from(sector - 1);
        Ok(index * self.sector_bytes)
    }
}

impl std::fmt::Display for RecordingGeometry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} cylinders of {} heads at {} sectors of {} bytes",
            self.cylinders, self.heads, self.sectors_per_track, self.sector_bytes
        )
    }
}

/// One source's own statement about the recording's coordinates.
///
/// A reading states the parts its source actually carries and leaves the
/// rest absent — a boot record records heads and sectors-per-track and
/// says nothing about how many cylinders the drive had. Nothing is
/// filled in from a neighbour.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeometryReading {
    /// Which enumerated source this reading came from.
    pub source: GeometrySource,
    /// Where in the artifact it was taken, in words fit to quote in a
    /// refusal a user reads.
    pub at: String,
    pub cylinders: Option<u32>,
    pub heads: Option<u32>,
    pub sectors_per_track: Option<u32>,
    pub sector_bytes: Option<u64>,
    /// What the source states, in its own terms (P4).
    pub detail: String,
}

impl GeometryReading {
    fn new(source: GeometrySource, at: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            source,
            at: at.into(),
            cylinders: None,
            heads: None,
            sectors_per_track: None,
            sector_bytes: None,
            detail: detail.into(),
        }
    }

    /// Whether this reading states any part of the coordinates at all.
    fn states_anything(&self) -> bool {
        self.cylinders.is_some()
            || self.heads.is_some()
            || self.sectors_per_track.is_some()
            || self.sector_bytes.is_some()
    }
}

/// A medium's geometry as the evidence left it: what was settled, what
/// disagreed, and every reading taken.
///
/// It is established when the medium is loaded and is evidence from then
/// on — nothing re-reads a boot record behind a caller, and nothing is
/// declared onto it afterwards.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Geometry {
    determined: Option<RecordingGeometry>,
    conflicts: Vec<String>,
    unsettled: Vec<&'static str>,
    readings: Vec<GeometryReading>,
}

impl Geometry {
    /// The geometry of a medium no source states one for — an archive,
    /// whose content is reached by name and has no coordinates at all.
    ///
    /// It is the ordinary settling of nothing rather than a special
    /// value: no reading, so no part settled and nothing in conflict.
    pub(crate) fn unstated() -> Self {
        settle(Vec::new(), 0)
    }

    /// The geometry of a medium the author stated one for — the third
    /// fact class arriving where the other two never do.
    ///
    /// There is nothing to settle: authorship states every part at once
    /// or it is not a geometry at all (the creation verb refuses a
    /// partial one), and no second source exists to disagree with, an
    /// authored medium having no artifact beneath it to read. The one
    /// reading says so, and says who stated it (P4).
    pub(crate) fn authored(coordinates: RecordingGeometry) -> Self {
        let mut reading = GeometryReading::new(
            GeometrySource::Authorship,
            "the author's own statement at new_media",
            format!(
                "the author created this medium whole and stated {coordinates}; \
                 nothing was read off an artifact, there being none"
            ),
        );
        reading.cylinders = Some(coordinates.cylinders);
        reading.heads = Some(coordinates.heads);
        reading.sectors_per_track = Some(coordinates.sectors_per_track);
        reading.sector_bytes = Some(coordinates.sector_bytes);
        Self {
            determined: Some(coordinates),
            conflicts: Vec::new(),
            unsettled: Vec::new(),
            readings: vec![reading],
        }
    }

    /// What the evidence established.
    pub fn state(&self) -> GeometryState {
        if self.determined.is_some() {
            GeometryState::Determined
        } else if self.conflicts.is_empty() {
            GeometryState::Unstated
        } else {
            GeometryState::Undetermined
        }
    }

    /// The coordinates, where the evidence settled them, and `None`
    /// where it did not. Absence is an answer: `state` says which of the
    /// two absences this is.
    pub fn determined(&self) -> Option<RecordingGeometry> {
        self.determined
    }

    /// Every reading taken, in the order the sources were read, whether
    /// or not they agreed. This is the whole of what the artifact said
    /// about its own coordinates.
    pub fn readings(&self) -> &[GeometryReading] {
        &self.readings
    }

    /// One sentence per part of the coordinates the sources contradict
    /// each other about, each naming both readings. Empty where nothing
    /// disagrees, which is true of a determined geometry and of one
    /// nothing states.
    pub fn conflicts(&self) -> &[String] {
        &self.conflicts
    }

    /// Which parts of the coordinates no source settled — because
    /// nothing stated one, or because two sources disagreed — in the
    /// order they are stated. Empty for a determined geometry.
    pub fn unsettled(&self) -> &[&'static str] {
        &self.unsettled
    }

    /// The coordinates a sector verb addresses in, or the refusal naming
    /// the evidence state that has none to offer.
    pub(crate) fn require(&self, named: &str) -> Result<RecordingGeometry> {
        if let Some(geometry) = self.determined {
            return Ok(geometry);
        }
        match self.state() {
            GeometryState::Undetermined => Err(refuse(
                GeometryRule::GeometryUndetermined,
                format!(
                    "the sources beneath {named} disagree about its geometry \
                     and nothing here settles it, so there are no coordinates \
                     to address in: {}",
                    self.conflicts.join("; ")
                ),
            )),
            _ => Err(refuse(
                GeometryRule::GeometryUnstated,
                format!(
                    "nothing beneath {named} states {}, so it has no \
                     coordinates to address in; every reading taken is on the \
                     medium's geometry",
                    match self.unsettled.as_slice() {
                        [] => "a geometry".to_owned(),
                        parts => parts.join(", "),
                    }
                ),
            )),
        }
    }
}

// The parts of the coordinates, named once — the conflict sentences and
// the unsettled list are the same words about the same fields.
const CYLINDERS: &str = "the cylinder count";
const HEADS: &str = "the head count";
const SECTORS_PER_TRACK: &str = "the sectors per track";
const SECTOR_BYTES: &str = "the sector size";

/// What the establishment reads, gathered by the caller that holds the
/// medium's own facts.
pub(crate) struct GeometrySources<'a> {
    /// The recognized image format's stable spelling, for the reading's
    /// own account of itself.
    pub(crate) format_id: &'static str,
    /// The geometry that format declares for every image it claims,
    /// where it declares one.
    pub(crate) format_disk: Option<DiskDescriptor>,
    /// The block size the load declared, where the format records none
    /// of its own — the raw reading, and nothing else.
    pub(crate) declared_sector_bytes: Option<u64>,
    /// Whether a partition table was read: the scheme's end tuples are a
    /// source only where the medium is laid out under one.
    pub(crate) reads_table: bool,
    /// Where each addressable partition starts, by ordinal — the
    /// positions a boot record could be at.
    pub(crate) boot_records: &'a [(u32, u64)],
    /// The presented content's own size, which is what extent arithmetic
    /// works over.
    pub(crate) extent_bytes: u64,
}

/// Reads every source that speaks and settles what they agree on.
///
/// A source that cannot be read states nothing, and the establishment
/// answers whatever the others left: a load never fails because a
/// geometry could not be established, because a geometry is evidence
/// about the artifact rather than a condition of opening it.
pub(crate) fn establish(device: &mut dyn Device, sources: GeometrySources<'_>) -> Geometry {
    let mut readings = Vec::new();

    if let Some(disk) = sources.format_disk {
        let mut reading = GeometryReading::new(
            GeometrySource::FormatDeclaration,
            format!("the {} format's own declaration", sources.format_id),
            format!(
                "the {} adapter records {} cylinders of {} side(s) at {} \
                 sectors of {} bytes for every image it claims",
                sources.format_id,
                disk.cylinders,
                disk.sides,
                disk.sectors_per_track,
                disk.sector_size
            ),
        );
        reading.cylinders = Some(disk.cylinders);
        reading.heads = Some(disk.sides);
        reading.sectors_per_track = Some(disk.sectors_per_track);
        reading.sector_bytes = Some(disk.sector_size);
        readings.push(reading);
    } else if let Some(sector_bytes) = sources.declared_sector_bytes {
        let mut reading = GeometryReading::new(
            GeometrySource::FormatDeclaration,
            "the load's own declaration of the block size",
            format!(
                "the {} reading records no addressable unit, so the load \
                 declared {sector_bytes}-byte blocks",
                sources.format_id
            ),
        );
        reading.sector_bytes = Some(sector_bytes);
        readings.push(reading);
    }

    if sources.reads_table {
        let mut unit = GeometryReading::new(
            GeometrySource::PartitionTable,
            "the partition table's own addressing unit",
            format!(
                "the table's extents are numbered in {}-byte blocks, which is \
                 what this release's reading of the scheme is written against",
                mbr::TABLE_BLOCK_BYTES
            ),
        );
        unit.sector_bytes = Some(mbr::TABLE_BLOCK_BYTES);
        readings.push(unit);

        for implied in mbr::implied_geometry(device) {
            let mut reading = GeometryReading::new(
                GeometrySource::PartitionTable,
                format!("the end tuple of partition {}", implied.entry),
                implied.detail,
            );
            reading.heads = Some(implied.heads);
            reading.sectors_per_track = Some(implied.sectors_per_track);
            readings.push(reading);
        }
    }

    for &(ordinal, start) in sources.boot_records {
        let mut sector = [0u8; 512];
        if device.read_at(start, &mut sector).is_err() {
            continue;
        }
        let Some(declared) = fat::declared_geometry(&sector) else {
            continue;
        };
        let mut reading = GeometryReading::new(
            GeometrySource::BootRecord,
            format!("the boot record at partition {ordinal}"),
            format!(
                "the boot record records {} bytes per sector, {}, and {}",
                declared.bytes_per_sector,
                match declared.sectors_per_track {
                    Some(sectors) => format!("{sectors} sectors per track"),
                    None => "no sectors-per-track".to_owned(),
                },
                match declared.heads {
                    Some(heads) => format!("{heads} heads"),
                    None => "no head count".to_owned(),
                }
            ),
        );
        reading.sector_bytes = Some(declared.bytes_per_sector);
        reading.sectors_per_track = declared.sectors_per_track;
        reading.heads = declared.heads;
        if reading.states_anything() {
            readings.push(reading);
        }
    }

    settle(readings, sources.extent_bytes)
}

/// Settles the readings against each other, deriving the cylinder count
/// from the extent under whatever track geometry they fixed.
fn settle(mut readings: Vec<GeometryReading>, extent_bytes: u64) -> Geometry {
    let mut conflicts = Vec::new();
    let sector_bytes = settle_part(SECTOR_BYTES, &readings, |r| r.sector_bytes, &mut conflicts);
    let heads = settle_part(HEADS, &readings, |r| r.heads.map(u64::from), &mut conflicts);
    let sectors_per_track = settle_part(
        SECTORS_PER_TRACK,
        &readings,
        |r| r.sectors_per_track.map(u64::from),
        &mut conflicts,
    );

    // Extent arithmetic is second-order by nature: an extent states a
    // cylinder count only once something else has fixed how big a
    // cylinder is, so it is read after the others and never against a
    // track geometry it had to guess at.
    if let (Some(sector_bytes), Some(heads), Some(sectors_per_track)) =
        (sector_bytes, heads, sectors_per_track)
    {
        if let Some(reading) = extent_reading(extent_bytes, sector_bytes, heads, sectors_per_track)
        {
            readings.push(reading);
        }
    }
    let cylinders = settle_part(
        CYLINDERS,
        &readings,
        |r| r.cylinders.map(u64::from),
        &mut conflicts,
    );

    let determined = match (cylinders, heads, sectors_per_track, sector_bytes) {
        (Some(cylinders), Some(heads), Some(sectors_per_track), Some(sector_bytes)) => {
            Some(RecordingGeometry {
                cylinders: cylinders as u32,
                heads: heads as u32,
                sectors_per_track: sectors_per_track as u32,
                sector_bytes,
            })
        }
        _ => None,
    };
    // A part is unsettled whether nothing stated it or two sources
    // disagreed about it: both leave nothing to address in, and the
    // conflicts say which of the two happened.
    let unsettled = [
        (CYLINDERS, cylinders),
        (HEADS, heads),
        (SECTORS_PER_TRACK, sectors_per_track),
        (SECTOR_BYTES, sector_bytes),
    ]
    .into_iter()
    .filter(|(_, settled)| settled.is_none())
    .map(|(part, _)| part)
    .collect();
    Geometry {
        determined,
        conflicts,
        unsettled,
        readings,
    }
}

/// One part of the coordinates, settled across every reading that states
/// it: the value where they agree, and a conflict sentence naming both
/// sides where they do not.
fn settle_part(
    part: &'static str,
    readings: &[GeometryReading],
    stated: impl Fn(&GeometryReading) -> Option<u64>,
    conflicts: &mut Vec<String>,
) -> Option<u64> {
    let stated: Vec<(u64, &GeometryReading)> = readings
        .iter()
        .filter_map(|reading| {
            stated(reading)
                .filter(|value| *value != 0)
                .map(|value| (value, reading))
        })
        .collect();
    let (first, from) = *stated.first()?;
    match stated
        .iter()
        .find(|(value, _)| *value != first)
        .map(|(value, reading)| (*value, *reading))
    {
        Some((other, elsewhere)) => {
            conflicts.push(format!(
                "{part}: {} states {first} and {} states {other} — both \
                 readings stand and neither settles it",
                from.at, elsewhere.at
            ));
            None
        }
        None => Some(first),
    }
}

/// What the content's own extent states about the cylinder count, under
/// a track geometry the other sources fixed.
fn extent_reading(
    extent_bytes: u64,
    sector_bytes: u64,
    heads: u64,
    sectors_per_track: u64,
) -> Option<GeometryReading> {
    let total_sectors = extent_bytes / sector_bytes;
    let per_cylinder = heads * sectors_per_track;
    if total_sectors == 0 {
        return None;
    }
    let cylinders = total_sectors.div_ceil(per_cylinder);
    if cylinders > u64::from(u32::MAX) {
        return None;
    }
    let over = total_sectors % per_cylinder;
    let mut reading = GeometryReading::new(
        GeometrySource::ExtentArithmetic,
        "the content's own extent",
        match over {
            0 => format!(
                "{extent_bytes} bytes hold {total_sectors} sectors of \
                 {sector_bytes} bytes, which is exactly {cylinders} cylinders \
                 of {per_cylinder} sectors"
            ),
            over => format!(
                "{extent_bytes} bytes hold {total_sectors} sectors of \
                 {sector_bytes} bytes, which is {} whole cylinders of \
                 {per_cylinder} sectors and {over} sectors into cylinder {}: \
                 the recording spans {cylinders} cylinders and the content \
                 does not hold all of the last one",
                cylinders - 1,
                cylinders - 1
            ),
        },
    );
    reading.cylinders = Some(cylinders as u32);
    Some(reading)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reading(
        source: GeometrySource,
        at: &str,
        cylinders: Option<u32>,
        heads: Option<u32>,
        sectors_per_track: Option<u32>,
        sector_bytes: Option<u64>,
    ) -> GeometryReading {
        let mut reading = GeometryReading::new(source, at, "stated for the test");
        reading.cylinders = cylinders;
        reading.heads = heads;
        reading.sectors_per_track = sectors_per_track;
        reading.sector_bytes = sector_bytes;
        reading
    }

    fn boot_record(heads: u32, sectors_per_track: u32) -> GeometryReading {
        reading(
            GeometrySource::BootRecord,
            "the boot record at partition 1",
            None,
            Some(heads),
            Some(sectors_per_track),
            Some(512),
        )
    }

    fn end_tuple(heads: u32, sectors_per_track: u32) -> GeometryReading {
        reading(
            GeometrySource::PartitionTable,
            "the end tuple of partition 1",
            None,
            Some(heads),
            Some(sectors_per_track),
            None,
        )
    }

    #[test]
    fn every_source_state_and_rule_round_trips_through_its_spelling() {
        for source in GeometrySource::ALL {
            assert_eq!(GeometrySource::from_id(source.as_str()), Some(source));
            assert!(!source.as_str().is_empty());
        }
        assert_eq!(GeometrySource::from_id("guesswork"), None);
        for rule in GeometryRule::ALL {
            assert_eq!(GeometryRule::from_identity(rule.as_str()), Some(rule));
        }
        assert_eq!(GeometryRule::from_identity("partition-no-extent"), None);
        for state in GeometryState::ALL {
            assert!(!state.as_str().is_empty());
        }
    }

    #[test]
    fn agreeing_sources_settle_the_coordinates_and_the_extent_states_cylinders() {
        // 8 heads of 32 sectors over 4,194,304 bytes: 8192 sectors, which
        // is exactly 32 cylinders of 256.
        let geometry = settle(vec![boot_record(8, 32), end_tuple(8, 32)], 4_194_304);
        assert_eq!(geometry.state(), GeometryState::Determined);
        assert_eq!(
            geometry.determined(),
            Some(RecordingGeometry {
                cylinders: 32,
                heads: 8,
                sectors_per_track: 32,
                sector_bytes: 512,
            })
        );
        assert!(geometry.conflicts().is_empty());
        assert!(geometry.unsettled().is_empty());
        let extent = geometry
            .readings()
            .iter()
            .find(|reading| reading.source == GeometrySource::ExtentArithmetic)
            .expect("the extent states the cylinder count");
        assert!(
            extent.detail.contains("exactly 32 cylinders"),
            "{}",
            extent.detail
        );
    }

    #[test]
    fn sources_that_disagree_settle_nothing_and_both_readings_stand() {
        let geometry = settle(vec![boot_record(16, 63), end_tuple(15, 63)], 4_194_304);
        assert_eq!(geometry.state(), GeometryState::Undetermined);
        assert_eq!(geometry.determined(), None);
        assert_eq!(geometry.conflicts().len(), 1, "one part disagrees");
        let conflict = &geometry.conflicts()[0];
        assert!(
            conflict.contains("16") && conflict.contains("15"),
            "{conflict}"
        );
        assert!(
            conflict.contains("boot record") && conflict.contains("end tuple"),
            "the sentence names both readings: {conflict}"
        );
        assert_eq!(geometry.readings().len(), 2, "both readings stand");
        assert!(
            geometry.unsettled().contains(&HEADS),
            "the head count is what nothing settled"
        );
        // The parts nothing disagreed about are still not *settled*
        // enough to address in: a geometry is whole or it is nothing.
        let error = geometry.require("this medium").expect_err("no coordinates");
        assert_eq!(
            error.rule(),
            Some(GeometryRule::GeometryUndetermined.as_str())
        );
        assert!(error.to_string().contains("neither settles it"), "{error}");
    }

    #[test]
    fn a_partial_geometry_is_unstated_rather_than_a_record_with_holes() {
        // A boot record with no track geometry states the sector size and
        // nothing else, so the extent has no cylinder to compute.
        let geometry = settle(
            vec![reading(
                GeometrySource::BootRecord,
                "the boot record at partition 0",
                None,
                None,
                None,
                Some(512),
            )],
            4_194_304,
        );
        assert_eq!(geometry.state(), GeometryState::Unstated);
        assert_eq!(
            geometry.unsettled(),
            vec![CYLINDERS, HEADS, SECTORS_PER_TRACK]
        );
        let error = geometry.require("this medium").expect_err("no coordinates");
        assert_eq!(error.rule(), Some(GeometryRule::GeometryUnstated.as_str()));
        assert!(
            error.to_string().contains("the head count"),
            "the refusal names what is missing: {error}"
        );
    }

    #[test]
    fn an_authored_geometry_is_the_authors_one_reading_and_settles_by_construction() {
        let coordinates = RecordingGeometry {
            cylinders: 1024,
            heads: 16,
            sectors_per_track: 63,
            sector_bytes: 512,
        };
        let geometry = Geometry::authored(coordinates);
        assert_eq!(geometry.state(), GeometryState::Determined);
        assert_eq!(geometry.determined(), Some(coordinates));
        assert!(geometry.conflicts().is_empty());
        assert!(geometry.unsettled().is_empty());
        assert_eq!(
            geometry.readings().len(),
            1,
            "authorship states every part at once and has no second source \
             to agree or disagree with"
        );
        assert_eq!(geometry.readings()[0].source, GeometrySource::Authorship);
        assert!(
            geometry.readings()[0].detail.contains("there being none"),
            "the reading says nothing was read: {}",
            geometry.readings()[0].detail
        );
        assert_eq!(
            geometry.require("this authored medium").expect("stated"),
            coordinates
        );
        assert_eq!(coordinates.checked_total_bytes(), Some(528_482_304));
        assert_eq!(
            RecordingGeometry {
                cylinders: u32::MAX,
                heads: u32::MAX,
                sectors_per_track: u32::MAX,
                sector_bytes: u64::MAX,
            }
            .checked_total_bytes(),
            None,
            "coordinates that came from nowhere but a caller may not fit"
        );
    }

    #[test]
    fn nothing_stated_at_all_is_unstated_and_carries_no_readings() {
        let geometry = Geometry::unstated();
        assert_eq!(geometry.state(), GeometryState::Unstated);
        assert!(geometry.readings().is_empty());
        assert!(geometry.conflicts().is_empty());
        assert!(geometry.require("this archive").is_err());
    }

    #[test]
    fn an_extent_short_of_a_whole_cylinder_says_so_and_still_spans_it() {
        // 4,096,000 bytes at 16 heads of 63 sectors: 8000 sectors, which
        // is 7 whole cylinders of 1008 and 944 sectors into the eighth.
        let geometry = settle(vec![boot_record(16, 63)], 4_096_000);
        let determined = geometry.determined().expect("settled");
        assert_eq!(determined.cylinders, 8, "the recording spans eight");
        let extent = &geometry.readings()[geometry.readings().len() - 1];
        assert!(
            extent.detail.contains("does not hold all of the last one"),
            "{}",
            extent.detail
        );
        // The geometry addresses more than the content holds, which is
        // exactly why the sector verbs check the extent as well.
        assert!(determined.total_bytes() > 4_096_000);
    }

    #[test]
    fn a_coordinate_outside_the_geometry_is_refused_naming_it() {
        let geometry = RecordingGeometry {
            cylinders: 40,
            heads: 1,
            sectors_per_track: 10,
            sector_bytes: 256,
        };
        assert_eq!(geometry.offset_of(0, 0, 1).expect("the first sector"), 0);
        assert_eq!(
            geometry.offset_of(0, 0, 10).expect("the last of track 0"),
            2_304
        );
        assert_eq!(geometry.offset_of(1, 0, 1).expect("track 1"), 2_560);
        assert_eq!(geometry.total_sectors(), 400);
        assert_eq!(geometry.total_bytes(), 102_400);

        for (cylinder, head, sector) in [(40, 0, 1), (0, 1, 1), (0, 0, 11), (0, 0, 0)] {
            let error = geometry
                .offset_of(cylinder, head, sector)
                .expect_err("outside the coordinates");
            assert_eq!(error.rule(), Some(GeometryRule::OutsideGeometry.as_str()));
            assert!(
                error
                    .to_string()
                    .contains("40 cylinders of 1 heads at 10 sectors"),
                "the refusal names the geometry: {error}"
            );
        }
    }

    #[test]
    fn sectors_number_from_one_because_the_recording_numbers_them_that_way() {
        let geometry = RecordingGeometry {
            cylinders: 2,
            heads: 2,
            sectors_per_track: 4,
            sector_bytes: 512,
        };
        // Cylinder 0 head 1 sector 1 is the fifth sector of the disk, not
        // the fourth: heads advance inside a cylinder.
        assert_eq!(geometry.offset_of(0, 1, 1).expect("second head"), 4 * 512);
        assert_eq!(
            geometry.offset_of(1, 0, 1).expect("second cylinder"),
            8 * 512
        );
        assert!(geometry.offset_of(0, 0, 0).is_err(), "there is no sector 0");
    }
}
