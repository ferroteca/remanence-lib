// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The file-container presentation: the interface every file-bearing
//! provider presents through at the P19 seam, and the vocabulary they
//! answer in.
//!
//! **There is no file-container layer here.** P19 makes file containers
//! a seam: adapters *expose* a view and results *present* an interface.
//! A ZIP grammar, a FAT volume, a Commodore directory and a composed
//! namespace are real systems that already hold their own structure,
//! and each of them can present a view of it. Nothing in this module
//! materializes a container above them — there is no intermediate
//! representation to copy into, and so no second place a listing lives
//! and nothing to invalidate when the truth beneath changes.
//!
//! That is what keeps reads bounded (P27): a provider answers for the
//! container it was asked about by reading that container, rather than
//! building an item pool for fifty thousand files before the caller can
//! list one of them.
//!
//! Every presentation is a view *of* something — the lowest durable
//! layer the session has materialized, which is the source of truth and
//! which a presentation never is. That floor may be an archive's own
//! named-entry state, addressed CHS records, logical blocks, or timed
//! flux. Each item's [`FloorExtent`] hook says where it sits in the
//! floor's own addressing, and nothing is ever written through a
//! presentation.
//!
//! Two rules shape the vocabulary:
//!
//! - **Names and items are different things.** [`entries`] returns
//!   names reaching targets, so one item may be reached by several
//!   names, a flat filesystem is one root whose entries all reach
//!   leaves, and hierarchy is a container whose entries reach
//!   containers. Listing order is evidence, carried as each entry's
//!   ordinal; nothing here re-sorts a source's listing.
//! - **The unclaimed remainder is itemized, never named.** In-force P19
//!   refuses to manufacture pseudo-files, so an [`ItemKind::OpaqueRegion`]
//!   is reached through the account rather than by path. That is what
//!   lets a view hold holes without lying in either direction: a
//!   presentation may be incomplete precisely because it is not the
//!   truth.
//!
//! Metadata follows the two-outcome rule the flux layer established one
//! seam down: a source fact either maps to a named field here, or is
//! retained as a [`DeclaredFact`] under its provider's namespace with
//! its source spelling and order intact. Nothing is normalized on the
//! way through — a timestamp keeps its source precision, epoch and zone
//! semantics, because the provider's namespace says what its value
//! means.
//!
//! [`entries`]: FileContainerView::entries

// The providers that will present through this contract arrive with
// their own features; until then it has no non-test implementors.
#![allow(dead_code)]

use std::fs::File;
use std::sync::Arc;

use crate::error::{Error, Result};

/// A provider's own identity for one item.
///
/// The value belongs to the provider that issued it — a directory
/// entry's location, a central-directory index, an inode — and means
/// nothing to anyone else. Nothing in this module assigns identities,
/// because nothing here holds a pool to index into. Callers pass a ref
/// back to the provider that gave it to them and never interpret it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct ItemRef(pub(crate) u64);

/// The encoding a provider claims for a recorded name.
///
/// The set grows additively as providers are admitted, exactly as the
/// flux layer's named homes do: a provider whose encoding has no
/// variant here keeps the name's bytes and does not claim an encoding
/// it has not implemented.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NameEncoding {
    Ascii,
    Utf8,
    Utf16Le,
    /// Commodore PETSCII, in the provider's claimed variant.
    Petscii,
    /// A DOS/Windows OEM code page, by its numeric identity.
    OemCodepage(u16),
}

/// How faithfully a decoded presentation represents the recorded bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NameConversion {
    /// Every byte mapped, and the presentation is reversible.
    Exact,
    /// Bytes the claimed encoding does not cover were substituted. The
    /// recorded bytes remain authoritative; this names what happened.
    Lossy { detail: String },
}

/// One name a source recorded, kept as bytes first.
///
/// The bytes are what the source stored and are never rewritten. The
/// decoded presentation sits beside them with the provenance of its
/// conversion, never in place of them: a ZIP name is UTF-8 only when
/// the grammar's own flag says so, and irregular names — trailing
/// spaces, shift characters, bytes outside the claimed encoding — stay
/// as recorded and are reported as issues rather than repaired.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RecordedName {
    pub(crate) bytes: Vec<u8>,
    pub(crate) encoding: NameEncoding,
    pub(crate) decoded: String,
    pub(crate) conversion: NameConversion,
}

impl RecordedName {
    pub(crate) fn new(
        bytes: impl Into<Vec<u8>>,
        encoding: NameEncoding,
        decoded: impl Into<String>,
        conversion: NameConversion,
    ) -> Self {
        Self {
            bytes: bytes.into(),
            encoding,
            decoded: decoded.into(),
            conversion,
        }
    }

    /// A name whose source bytes are already UTF-8, decoded exactly.
    pub(crate) fn utf8(name: impl Into<String>) -> Self {
        let decoded = name.into();
        Self {
            bytes: decoded.as_bytes().to_vec(),
            encoding: NameEncoding::Utf8,
            decoded,
            conversion: NameConversion::Exact,
        }
    }
}

/// One name in one container, reaching one item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Entry {
    pub(crate) name: RecordedName,
    pub(crate) target: ItemRef,
    /// The entry's position in the source's own listing order.
    pub(crate) ordinal: u64,
}

/// What an item is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ItemKind {
    /// A namespace level, whose entries name other items. It claims
    /// namespace structure, not allocation in the floor.
    Container,
    /// An item with extractable content, reached through its hook.
    File,
    /// The itemized remainder of the account: an extent of the floor
    /// this interpretation does not claim. It is never named, never a
    /// verdict about what the extent holds, and never a pseudo-file.
    OpaqueRegion,
}

impl ItemKind {
    /// The kind's stable spelling, for diagnostics.
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Container => "container",
            Self::File => "file",
            Self::OpaqueRegion => "opaque region",
        }
    }
}

/// What a provider's size claim is actually a claim *about*.
///
/// These are different claims, not one number: an exactly stored byte
/// count, a size expressed in allocation units, and a rounded count of
/// fixed-length records each say something different about the bytes a
/// read will produce.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SizeBasis {
    /// The source stores the byte length itself.
    Exact,
    /// The source states a count of allocation units only.
    AllocationUnits { unit_bytes: u64, units: u64 },
    /// The source states a count of fixed-length records, so the byte
    /// length is rounded to the record boundary (CP/M's 128-byte
    /// records).
    RecordCount { record_bytes: u64, records: u64 },
}

/// The byte size a provider claims for a file item, with its basis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SizeClaim {
    pub(crate) bytes: u64,
    pub(crate) basis: SizeBasis,
}

impl SizeClaim {
    pub(crate) const fn exact(bytes: u64) -> Self {
        Self {
            bytes,
            basis: SizeBasis::Exact,
        }
    }
}

/// Where a file item's bytes come from, bounded (P27).
///
/// This is a descriptor, never the bytes: a presentation is a map and
/// not a pipe, and reading resolves through the provider that owns the
/// floor.
pub(crate) enum ContentSource {
    /// A span of the claimed source artifact, read in place.
    InPlace { offset: u64, length: u64 },
    /// Decoded once into private session storage.
    Spooled { spool: Arc<File>, length: u64 },
    /// Assembled from extents of the floor, in the order given.
    Floor { extents: Vec<FloorExtent> },
    /// The item holds no bytes at all.
    Empty,
}

impl std::fmt::Debug for ContentSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InPlace { offset, length } => f
                .debug_struct("InPlace")
                .field("offset", offset)
                .field("length", length)
                .finish(),
            Self::Spooled { length, .. } => {
                f.debug_struct("Spooled").field("length", length).finish()
            }
            Self::Floor { extents } => f
                .debug_struct("Floor")
                .field("extents", &extents.len())
                .finish(),
            Self::Empty => f.write_str("Empty"),
        }
    }
}

/// What one addressable unit of the floor *is*.
///
/// A presentation has exactly one floor, so hooks and the account share
/// one unit ordering and totality is checkable. The descriptor keeps
/// the vocabulary's own spelling recoverable; extents index units in
/// it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FloorAddressing {
    /// Bytes of a serialized artifact — an archive's own named-entry
    /// state is presented over its file this way.
    Bytes,
    /// Geometry-opaque logical blocks numbered from zero.
    Blocks { bytes_per_block: u32 },
    /// Records addressed by cylinder, head and sector under a declared
    /// geometry, numbered in CHS order.
    Chs {
        heads: u32,
        sectors_per_track: u32,
        bytes_per_sector: u32,
    },
    /// Track-relative angular flux regions: units run through each
    /// track's revolution in turn.
    Flux { ticks_per_revolution: u64 },
}

impl FloorAddressing {
    /// The unit's stable spelling, for diagnostics.
    pub(crate) const fn unit(self) -> &'static str {
        match self {
            Self::Bytes => "byte",
            Self::Blocks { .. } => "block",
            Self::Chs { .. } => "record",
            Self::Flux { .. } => "flux unit",
        }
    }
}

/// The lowest durable layer the session has materialized: the truth a
/// presentation is a view of, and never is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Floor {
    pub(crate) addressing: FloorAddressing,
    /// The addressable extent the account must cover in full.
    pub(crate) total_units: u64,
}

/// A half-open run of addressable units in the floor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FloorExtent {
    pub(crate) start: u64,
    pub(crate) count: u64,
}

impl FloorExtent {
    pub(crate) const fn new(start: u64, count: u64) -> Self {
        Self { start, count }
    }

    /// The first unit past the extent, or `None` if the run overflows.
    pub(crate) fn end(&self) -> Option<u64> {
        self.start.checked_add(self.count)
    }
}

/// A fact a provider declares under its own namespace.
///
/// The value means whatever that namespace says it means. Nothing here
/// interprets or normalizes it, which is what lets a timestamp keep its
/// source precision, epoch and zone semantics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DeclaredFact {
    /// The provider namespace that owns the fact's meaning.
    pub(crate) namespace: &'static str,
    /// The key as the source spells it.
    pub(crate) key: String,
    pub(crate) value: FactValue,
    /// The fact's position in the source's own order.
    pub(crate) ordinal: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FactValue {
    Text(String),
    Bytes(Vec<u8>),
    Unsigned(u64),
    Signed(i64),
    Flag(bool),
}

/// A source structure retained verbatim because the vocabulary has no
/// named home for it yet.
///
/// Retaining it is the first of the two outcomes, not the permanent
/// one: a fact stays foreign only until a later revision gives it a
/// named field, which is what stops this becoming a blind spot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ForeignRecord {
    pub(crate) namespace: &'static str,
    pub(crate) type_id: String,
    pub(crate) version: Option<u32>,
    pub(crate) ordinal: u64,
    /// Where it sat in the floor, where the provider can say.
    pub(crate) source_range: Option<FloorExtent>,
    pub(crate) payload: Vec<u8>,
    /// Whatever the provider could safely decode, if anything.
    pub(crate) decoded_summary: Vec<DeclaredFact>,
}

/// Something qualified about an item, recorded rather than repaired.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Issue {
    /// The owning provider's stable spelling for this kind of issue.
    pub(crate) code: &'static str,
    pub(crate) detail: String,
}

impl Issue {
    pub(crate) fn new(code: &'static str, detail: impl Into<String>) -> Self {
        Self {
            code,
            detail: detail.into(),
        }
    }
}

/// How a fact came to be known.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Provenance {
    /// The provider namespace that established it.
    pub(crate) source: &'static str,
    /// Ordered notes recording how, in human-readable terms (P4).
    pub(crate) notes: Vec<String>,
}

impl Provenance {
    pub(crate) fn new(source: &'static str) -> Self {
        Self {
            source,
            notes: Vec::new(),
        }
    }

    pub(crate) fn note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }
}

/// Everything a provider claims about one item.
#[derive(Debug)]
pub(crate) struct ItemFacts {
    pub(crate) kind: ItemKind,
    /// Present for a file item; a claim, with what it is a claim about.
    pub(crate) size: Option<SizeClaim>,
    /// Where the item sits in the floor, in the floor's addressing.
    pub(crate) hook: Vec<FloorExtent>,
    pub(crate) facts: Vec<DeclaredFact>,
    pub(crate) issues: Vec<Issue>,
    pub(crate) provenance: Provenance,
}

impl ItemFacts {
    pub(crate) fn new(kind: ItemKind, provenance: Provenance) -> Self {
        Self {
            kind,
            size: None,
            hook: Vec::new(),
            facts: Vec::new(),
            issues: Vec::new(),
            provenance,
        }
    }
}

/// The view a file-bearing system presents at the P19 seam.
///
/// Implementors are the real systems themselves — an archive grammar, a
/// filesystem adapter, a namespace composer — answering about their own
/// structure. The shape is navigational rather than wholesale so that
/// answering one question costs one question's worth of reading (P27);
/// [`account`] is the deliberate exception, being a whole-artifact
/// report produced only when asked for.
///
/// [`account`]: FileContainerView::account
pub(crate) trait FileContainerView {
    /// The provider namespace, which attributes its refusals.
    fn source(&self) -> &'static str;

    /// The floor this is a view of.
    fn floor(&self) -> Floor;

    /// The namespace's roots, each a container. Several are ordinary:
    /// a composed namespace may have one per drive letter or mount.
    fn roots(&self) -> Result<Vec<ItemRef>>;

    /// The entries of one container, in the source's own order.
    ///
    /// An entry never targets an opaque region: the namespace lists
    /// only what the source names.
    fn entries(&self, container: ItemRef) -> Result<Vec<Entry>>;

    /// What the provider claims about one item.
    fn item(&self, item: ItemRef) -> Result<ItemFacts>;

    /// Where one file item's bytes come from, bounded.
    fn content(&self, item: ItemRef) -> Result<ContentSource>;

    /// The total account of the floor, computed on request.
    fn account(&self) -> Result<CoverageAccount>;
}

/// The refusal for an operation that needs a container.
pub(crate) fn not_a_container(source: &'static str, kind: ItemKind) -> Error {
    Error::invalid_image(source, format!("a {} holds no names", kind.as_str()))
}

/// The refusal for an item ref this provider did not issue.
pub(crate) fn no_such_item(source: &'static str, item: ItemRef) -> Error {
    Error::invalid_image(source, format!("this view holds no item {}", item.0))
}

/// The refusal for content asked of something that holds none.
pub(crate) fn not_a_file(source: &'static str, kind: ItemKind) -> Error {
    Error::invalid_image(
        source,
        format!("a {} has no extractable content", kind.as_str()),
    )
}

/// How one addressable unit of the floor is accounted for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CoverageClass {
    /// The data hook of an item the namespace names.
    ItemData(ItemRef),
    /// Structures the interpretation claims for itself — directory
    /// records, allocation metadata, boot and reserved areas, an
    /// archive's local headers and central directory. Deleted-but-
    /// present entries are accounted here, inside the structures they
    /// occupy; itemizing them would be a recovery claim.
    Structures,
    /// Space the allocation metadata claims is free. This records that
    /// claim and nothing else: not a verdict that the extent is empty,
    /// disposable, or safe to reuse.
    ClaimedFree,
    /// An extent the interpretation does not claim, itemized as the
    /// opaque region it names.
    Opaque(ItemRef),
}

impl CoverageClass {
    /// The class's stable spelling, for diagnostics.
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::ItemData(_) => "item data",
            Self::Structures => "interpretation structures",
            Self::ClaimedFree => "claimed-free space",
            Self::Opaque(_) => "opaque region",
        }
    }
}

/// One classified run of the floor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CoverageRegion {
    pub(crate) extent: FloorExtent,
    pub(crate) class: CoverageClass,
    /// Why the class was assigned, in human-readable terms (P4).
    pub(crate) evidence: Vec<String>,
}

/// A total, exclusive account of the floor.
///
/// Totality is true by construction rather than by assertion: the
/// provider claims what its interpretation covers and whatever remains
/// becomes opaque regions, so the regions are ordered by position and
/// together span exactly `[0, floor.total_units)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CoverageAccount {
    pub(crate) floor: Floor,
    pub(crate) regions: Vec<CoverageRegion>,
    /// The first ref assigned to an opaque region; refs run upward from
    /// it in position order.
    opaque_base: u64,
}

impl CoverageAccount {
    /// The regions whose class matches, in position order.
    pub(crate) fn regions_of(
        &self,
        matching: impl Fn(&CoverageClass) -> bool,
    ) -> impl Iterator<Item = &CoverageRegion> {
        self.regions
            .iter()
            .filter(move |region| matching(&region.class))
    }

    /// The number of units accounted to matching classes.
    pub(crate) fn units_of(&self, matching: impl Fn(&CoverageClass) -> bool) -> u64 {
        self.regions_of(matching)
            .map(|region| region.extent.count)
            .sum()
    }

    /// Whether `item` names one of this account's opaque regions.
    pub(crate) fn is_opaque(&self, item: ItemRef) -> bool {
        self.opaque_region(item).is_some()
    }

    /// The opaque region `item` names, if it names one.
    pub(crate) fn opaque_region(&self, item: ItemRef) -> Option<&CoverageRegion> {
        self.regions
            .iter()
            .find(|region| region.class == CoverageClass::Opaque(item))
    }

    /// The facts of an opaque region, so a provider can answer
    /// [`FileContainerView::item`] for one without tracking it itself.
    pub(crate) fn opaque_facts(&self, item: ItemRef, source: &'static str) -> Option<ItemFacts> {
        let region = self.opaque_region(item)?;
        let mut facts = ItemFacts::new(
            ItemKind::OpaqueRegion,
            Provenance::new(source).note("the interpretation claims no structure over this extent"),
        );
        facts.hook = vec![region.extent];
        Some(facts)
    }
}

/// Builds a [`CoverageAccount`] by claiming what an interpretation
/// covers and deriving the rest.
///
/// Claims are checked as they arrive (P6): an extent outside the floor,
/// an empty run, or an overlap with an existing claim is refused there
/// and then, naming both sides, rather than producing an account that
/// quietly contradicts itself.
pub(crate) struct CoverageBuilder {
    source: &'static str,
    floor: Floor,
    opaque_base: u64,
    /// Kept ordered by `extent.start`, and disjoint by construction.
    claimed: Vec<CoverageRegion>,
}

impl CoverageBuilder {
    /// Begins an account of `floor`.
    ///
    /// `opaque_base` is the first [`ItemRef`] value the builder may
    /// assign to a derived opaque region; the provider chooses a range
    /// that cannot collide with the refs it issues itself.
    pub(crate) fn new(source: &'static str, floor: Floor, opaque_base: u64) -> Self {
        Self {
            source,
            floor,
            opaque_base,
            claimed: Vec::new(),
        }
    }

    /// Claims `extent` for `class`, with the evidence for that reading.
    pub(crate) fn claim(
        &mut self,
        extent: FloorExtent,
        class: CoverageClass,
        evidence: Vec<String>,
    ) -> Result<()> {
        let unit = self.floor.addressing.unit();
        if extent.count == 0 {
            return Err(self.refuse(format!(
                "coverage claim at {unit} {} spans nothing; an item that occupies \
                 no {unit}s carries no extent at all",
                extent.start
            )));
        }
        let end = extent.end().ok_or_else(|| {
            self.refuse(format!(
                "coverage claim at {unit} {} for {} {unit}s runs past the end of \
                 the unit space",
                extent.start, extent.count
            ))
        })?;
        if end > self.floor.total_units {
            return Err(self.refuse(format!(
                "coverage claim covers {unit}s {}..{end}, past the floor's {} {unit}s",
                extent.start, self.floor.total_units
            )));
        }

        let position = self
            .claimed
            .partition_point(|region| region.extent.start < extent.start);
        if let Some(previous) = position.checked_sub(1).and_then(|at| self.claimed.get(at))
            && previous
                .extent
                .end()
                .is_some_and(|prior_end| prior_end > extent.start)
        {
            return Err(self.overlap(previous, extent));
        }
        if let Some(next) = self.claimed.get(position)
            && next.extent.start < end
        {
            return Err(self.overlap(next, extent));
        }

        self.claimed.insert(
            position,
            CoverageRegion {
                extent,
                class,
                evidence,
            },
        );
        Ok(())
    }

    /// Derives the opaque remainder and returns the finished account.
    ///
    /// Every gap between claims becomes one opaque region, so the
    /// account spans the floor exactly.
    pub(crate) fn finish(self) -> CoverageAccount {
        let unit = self.floor.addressing.unit();
        let mut regions: Vec<CoverageRegion> = Vec::with_capacity(self.claimed.len() + 1);
        let mut opaque_next = self.opaque_base;
        let mut position = 0_u64;

        let mut derive = |extent: FloorExtent, regions: &mut Vec<CoverageRegion>| {
            let item = ItemRef(opaque_next);
            opaque_next += 1;
            regions.push(CoverageRegion {
                extent,
                class: CoverageClass::Opaque(item),
                evidence: vec![format!(
                    "{unit}s {}..{} are covered by no claim of this interpretation",
                    extent.start,
                    extent.end().unwrap_or(u64::MAX)
                )],
            });
        };

        for region in self.claimed {
            if region.extent.start > position {
                derive(
                    FloorExtent::new(position, region.extent.start - position),
                    &mut regions,
                );
            }
            position = region.extent.end().expect("claims were bounds-checked");
            regions.push(region);
        }
        if position < self.floor.total_units {
            derive(
                FloorExtent::new(position, self.floor.total_units - position),
                &mut regions,
            );
        }

        CoverageAccount {
            floor: self.floor,
            regions,
            opaque_base: self.opaque_base,
        }
    }

    fn overlap(&self, existing: &CoverageRegion, incoming: FloorExtent) -> Error {
        let unit = self.floor.addressing.unit();
        self.refuse(format!(
            "coverage claim covering {unit}s {}..{} overlaps the {} already claimed \
             at {unit}s {}..{}",
            incoming.start,
            incoming.end().unwrap_or(u64::MAX),
            existing.class.as_str(),
            existing.extent.start,
            existing.extent.end().unwrap_or(u64::MAX),
        ))
    }

    fn refuse(&self, reason: impl Into<String>) -> Error {
        Error::invalid_image(self.source, reason)
    }
}

/// Checks one presentation against the contract's invariants.
///
/// This is a conformance harness for tests and for a new provider's own
/// verification, not a runtime path: it walks the whole namespace, which
/// is exactly what the interface exists to avoid doing during ordinary
/// use. It proves what the interface promises and a provider could
/// otherwise get wrong on its own: that the account is total and
/// exclusive over the declared floor, and that no entry names an opaque
/// region.
pub(crate) fn check_conformance(view: &dyn FileContainerView) -> Result<()> {
    let source = view.source();
    let floor = view.floor();
    let account = view.account()?;

    if account.floor != floor {
        return Err(Error::invalid_image(
            source,
            "the account describes a different floor than the view declares",
        ));
    }

    let unit = floor.addressing.unit();
    let mut position = 0_u64;
    for region in &account.regions {
        if region.extent.start != position {
            return Err(Error::invalid_image(
                source,
                format!(
                    "the account skips or repeats at {unit} {position}: the next \
                     region starts at {}",
                    region.extent.start
                ),
            ));
        }
        position = region.extent.end().ok_or_else(|| {
            Error::invalid_image(
                source,
                format!("an account region overflows at {unit} {position}"),
            )
        })?;
    }
    if position != floor.total_units {
        return Err(Error::invalid_image(
            source,
            format!(
                "the account covers {position} of the floor's {} {unit}s",
                floor.total_units
            ),
        ));
    }

    let mut pending = view.roots()?;
    let mut seen: Vec<ItemRef> = Vec::new();
    while let Some(item) = pending.pop() {
        if seen.contains(&item) {
            continue;
        }
        seen.push(item);
        let facts = view.item(item)?;
        if facts.kind != ItemKind::Container {
            continue;
        }
        for entry in view.entries(item)? {
            if account.is_opaque(entry.target) {
                return Err(Error::invalid_image(
                    source,
                    format!(
                        "the entry '{}' names an opaque region; the namespace lists \
                         only what the source names",
                        entry.name.decoded
                    ),
                ));
            }
            pending.push(entry.target);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCategory;

    const PROVIDER: &str = "test-provider";
    const OPAQUE_BASE: u64 = 1 << 32;

    /// A synthetic system that presents a view of its own structure —
    /// the contract's test surface, standing in for a real grammar.
    struct SyntheticProvider {
        floor: Floor,
        /// `(kind, size, hook, entries)` by ref value.
        items: Vec<(ItemKind, Option<SizeClaim>, Vec<FloorExtent>, Vec<Entry>)>,
        roots: Vec<ItemRef>,
        claims: Vec<(FloorExtent, CoverageClass)>,
    }

    impl SyntheticProvider {
        fn new(total_units: u64) -> Self {
            Self {
                floor: Floor {
                    addressing: FloorAddressing::Blocks {
                        bytes_per_block: 256,
                    },
                    total_units,
                },
                items: Vec::new(),
                roots: Vec::new(),
                claims: Vec::new(),
            }
        }

        fn bytes_floor(total_units: u64) -> Self {
            let mut provider = Self::new(total_units);
            provider.floor = Floor {
                addressing: FloorAddressing::Bytes,
                total_units,
            };
            provider
        }

        fn add(&mut self, kind: ItemKind, hook: Vec<FloorExtent>) -> ItemRef {
            self.items.push((kind, None, hook, Vec::new()));
            ItemRef(self.items.len() as u64 - 1)
        }

        fn add_file(&mut self, size: SizeClaim, hook: Vec<FloorExtent>) -> ItemRef {
            let item = self.add(ItemKind::File, hook);
            self.items[item.0 as usize].1 = Some(size);
            item
        }

        fn link(&mut self, parent: ItemRef, name: RecordedName, target: ItemRef) {
            let ordinal = self.items[parent.0 as usize].3.len() as u64;
            self.items[parent.0 as usize].3.push(Entry {
                name,
                target,
                ordinal,
            });
        }

        fn claim(&mut self, extent: FloorExtent, class: CoverageClass) {
            self.claims.push((extent, class));
        }

        fn entry_names(&self, container: ItemRef) -> Vec<String> {
            self.entries(container)
                .expect("the container exists")
                .into_iter()
                .map(|entry| entry.name.decoded)
                .collect()
        }
    }

    impl FileContainerView for SyntheticProvider {
        fn source(&self) -> &'static str {
            PROVIDER
        }

        fn floor(&self) -> Floor {
            self.floor
        }

        fn roots(&self) -> Result<Vec<ItemRef>> {
            Ok(self.roots.clone())
        }

        fn entries(&self, container: ItemRef) -> Result<Vec<Entry>> {
            let item = self
                .items
                .get(container.0 as usize)
                .ok_or_else(|| no_such_item(PROVIDER, container))?;
            if item.0 != ItemKind::Container {
                return Err(not_a_container(PROVIDER, item.0));
            }
            Ok(item.3.clone())
        }

        fn item(&self, item: ItemRef) -> Result<ItemFacts> {
            if let Some(facts) = self.account()?.opaque_facts(item, PROVIDER) {
                return Ok(facts);
            }
            let held = self
                .items
                .get(item.0 as usize)
                .ok_or_else(|| no_such_item(PROVIDER, item))?;
            let mut facts = ItemFacts::new(held.0, Provenance::new(PROVIDER));
            facts.size = held.1;
            facts.hook = held.2.clone();
            Ok(facts)
        }

        fn content(&self, item: ItemRef) -> Result<ContentSource> {
            let held = self
                .items
                .get(item.0 as usize)
                .ok_or_else(|| no_such_item(PROVIDER, item))?;
            if held.0 != ItemKind::File {
                return Err(not_a_file(PROVIDER, held.0));
            }
            Ok(ContentSource::Floor {
                extents: held.2.clone(),
            })
        }

        fn account(&self) -> Result<CoverageAccount> {
            let mut builder = CoverageBuilder::new(PROVIDER, self.floor, OPAQUE_BASE);
            for (extent, class) in &self.claims {
                builder.claim(*extent, *class, Vec::new())?;
            }
            Ok(builder.finish())
        }
    }

    #[test]
    fn one_item_may_be_reached_by_several_names() {
        let mut provider = SyntheticProvider::new(16);
        let root = provider.add(ItemKind::Container, Vec::new());
        provider.roots.push(root);
        let file = provider.add_file(SizeClaim::exact(12), vec![FloorExtent::new(4, 1)]);

        provider.link(root, RecordedName::utf8("README"), file);
        provider.link(root, RecordedName::utf8("readme.txt"), file);

        let entries = provider.entries(root).expect("the root is a container");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].target, entries[1].target);
        check_conformance(&provider).expect("the contract holds");
    }

    #[test]
    fn source_listing_order_is_preserved_as_evidence() {
        let mut provider = SyntheticProvider::new(16);
        let root = provider.add(ItemKind::Container, Vec::new());
        provider.roots.push(root);
        for name in ["ZEBRA", "alpha", "Middle"] {
            let file = provider.add_file(SizeClaim::exact(0), Vec::new());
            provider.link(root, RecordedName::utf8(name), file);
        }

        assert_eq!(provider.entry_names(root), ["ZEBRA", "alpha", "Middle"]);
        let ordinals: Vec<u64> = provider
            .entries(root)
            .expect("the root is a container")
            .iter()
            .map(|entry| entry.ordinal)
            .collect();
        assert_eq!(ordinals, [0, 1, 2]);
    }

    #[test]
    fn a_name_keeps_its_recorded_bytes_and_claimed_encoding() {
        let mut provider = SyntheticProvider::new(16);
        let root = provider.add(ItemKind::Container, Vec::new());
        let file = provider.add_file(SizeClaim::exact(2), Vec::new());
        // A CBM name padded with shifted spaces: the bytes are the
        // evidence, and the presentation never replaces them.
        provider.link(
            root,
            RecordedName::new(
                vec![0x50, 0x49, 0x43, 0xa0, 0xa0],
                NameEncoding::Petscii,
                "PIC",
                NameConversion::Lossy {
                    detail: "two trailing shifted spaces are not presentable".to_owned(),
                },
            ),
            file,
        );

        let entries = provider.entries(root).expect("the root is a container");
        let name = &entries[0].name;
        assert_eq!(name.bytes, [0x50, 0x49, 0x43, 0xa0, 0xa0]);
        assert_eq!(name.encoding, NameEncoding::Petscii);
        assert_eq!(name.decoded, "PIC");
        assert!(matches!(name.conversion, NameConversion::Lossy { .. }));
    }

    #[test]
    fn the_account_is_total_because_the_remainder_is_derived() {
        let mut provider = SyntheticProvider::new(100);
        let file = provider.add_file(SizeClaim::exact(2560), vec![FloorExtent::new(10, 10)]);
        provider.claim(FloorExtent::new(0, 4), CoverageClass::Structures);
        provider.claim(FloorExtent::new(10, 10), CoverageClass::ItemData(file));
        provider.claim(FloorExtent::new(20, 60), CoverageClass::ClaimedFree);

        let account = provider.account().expect("the account computes");
        let mut position = 0;
        for region in &account.regions {
            assert_eq!(region.extent.start, position);
            position = region.extent.end().expect("bounded");
        }
        assert_eq!(position, 100);
        assert_eq!(
            account.units_of(|class| matches!(class, CoverageClass::Opaque(_))),
            26,
            "blocks 4..10 and 80..100 belong to no claim"
        );
        check_conformance(&provider).expect("the contract holds");
    }

    #[test]
    fn an_archive_floor_accounts_its_unclaimed_bytes_the_same_way() {
        // A self-extractor stub ahead of the first local header is an
        // opaque region exactly as a protection track is.
        let mut provider = SyntheticProvider::bytes_floor(4096);
        let member = provider.add_file(SizeClaim::exact(512), vec![FloorExtent::new(2048, 512)]);
        provider.claim(FloorExtent::new(2048, 512), CoverageClass::ItemData(member));
        provider.claim(FloorExtent::new(2560, 1536), CoverageClass::Structures);

        let account = provider.account().expect("the account computes");
        let opaque: Vec<&CoverageRegion> = account
            .regions_of(|class| matches!(class, CoverageClass::Opaque(_)))
            .collect();
        assert_eq!(opaque.len(), 1);
        assert_eq!(opaque[0].extent, FloorExtent::new(0, 2048));
        assert!(
            opaque[0].evidence[0].contains("bytes 0..2048"),
            "{:?}",
            opaque[0]
        );
    }

    #[test]
    fn an_opaque_region_is_never_named_in_the_namespace() {
        let mut provider = SyntheticProvider::new(16);
        let root = provider.add(ItemKind::Container, Vec::new());
        provider.roots.push(root);
        provider.claim(FloorExtent::new(0, 4), CoverageClass::Structures);

        let account = provider.account().expect("the account computes");
        let opaque = account
            .regions_of(|class| matches!(class, CoverageClass::Opaque(_)))
            .next()
            .expect("the remainder was itemized");
        let CoverageClass::Opaque(opaque_ref) = opaque.class else {
            panic!("the class is opaque");
        };

        // It is a real item, reachable through the account...
        let facts = provider
            .item(opaque_ref)
            .expect("the account answers for it");
        assert_eq!(facts.kind, ItemKind::OpaqueRegion);
        assert_eq!(facts.hook, vec![FloorExtent::new(4, 12)]);
        check_conformance(&provider).expect("nothing names it");

        // ...and naming it is what the contract catches.
        provider.link(root, RecordedName::utf8("PROTECTION"), opaque_ref);
        let error = check_conformance(&provider).expect_err("the pseudo-file rule holds");
        assert_eq!(error.category(), ErrorCategory::InvalidImage);
        assert!(
            error
                .to_string()
                .contains("lists only what the source names"),
            "{error}"
        );
    }

    #[test]
    fn overlapping_claims_are_refused_naming_both_sides() {
        let mut builder = CoverageBuilder::new(
            PROVIDER,
            Floor {
                addressing: FloorAddressing::Blocks {
                    bytes_per_block: 256,
                },
                total_units: 50,
            },
            OPAQUE_BASE,
        );
        builder
            .claim(
                FloorExtent::new(10, 10),
                CoverageClass::Structures,
                Vec::new(),
            )
            .expect("in bounds");

        let error = builder
            .claim(
                FloorExtent::new(15, 10),
                CoverageClass::ClaimedFree,
                Vec::new(),
            )
            .expect_err("the extents overlap");
        assert_eq!(error.category(), ErrorCategory::InvalidImage);
        let message = error.to_string();
        assert!(message.contains("blocks 15..25"), "{message}");
        assert!(message.contains("interpretation structures"), "{message}");
        assert!(message.contains("blocks 10..20"), "{message}");
    }

    #[test]
    fn a_claim_past_the_floor_is_refused_in_the_floors_own_units() {
        let mut builder = CoverageBuilder::new(
            PROVIDER,
            Floor {
                addressing: FloorAddressing::Flux {
                    ticks_per_revolution: 3_200_000,
                },
                total_units: 32,
            },
            OPAQUE_BASE,
        );
        let error = builder
            .claim(
                FloorExtent::new(30, 8),
                CoverageClass::ClaimedFree,
                Vec::new(),
            )
            .expect_err("the claim runs past the end");
        assert!(
            error.to_string().contains("past the floor's 32 flux units"),
            "{error}"
        );
        assert!(
            builder
                .claim(
                    FloorExtent::new(0, 0),
                    CoverageClass::ClaimedFree,
                    Vec::new()
                )
                .is_err(),
            "an extent spanning nothing claims nothing"
        );
    }

    #[test]
    fn conformance_catches_an_account_that_does_not_cover_its_floor() {
        struct ShortAccount;

        impl FileContainerView for ShortAccount {
            fn source(&self) -> &'static str {
                PROVIDER
            }
            fn floor(&self) -> Floor {
                Floor {
                    addressing: FloorAddressing::Bytes,
                    total_units: 64,
                }
            }
            fn roots(&self) -> Result<Vec<ItemRef>> {
                Ok(Vec::new())
            }
            fn entries(&self, container: ItemRef) -> Result<Vec<Entry>> {
                Err(no_such_item(PROVIDER, container))
            }
            fn item(&self, item: ItemRef) -> Result<ItemFacts> {
                Err(no_such_item(PROVIDER, item))
            }
            fn content(&self, item: ItemRef) -> Result<ContentSource> {
                Err(no_such_item(PROVIDER, item))
            }
            fn account(&self) -> Result<CoverageAccount> {
                // Declares 64 bytes, accounts for 32.
                let mut builder = CoverageBuilder::new(
                    PROVIDER,
                    Floor {
                        addressing: FloorAddressing::Bytes,
                        total_units: 32,
                    },
                    OPAQUE_BASE,
                );
                builder.claim(
                    FloorExtent::new(0, 32),
                    CoverageClass::Structures,
                    Vec::new(),
                )?;
                Ok(builder.finish())
            }
        }

        let error = check_conformance(&ShortAccount).expect_err("the floors disagree");
        assert!(error.to_string().contains("different floor"), "{error}");
    }

    #[test]
    fn content_stays_a_bounded_descriptor() {
        let mut provider = SyntheticProvider::bytes_floor(96 * 1024 * 1024 * 1024);
        let huge = provider.add_file(
            SizeClaim {
                bytes: 96 * 1024 * 1024 * 1024,
                basis: SizeBasis::AllocationUnits {
                    unit_bytes: 4096,
                    units: 25_165_824,
                },
            },
            vec![FloorExtent::new(512, 96 * 1024 * 1024 * 1024 - 512)],
        );

        // Ninety-six gibibytes described without holding any of it.
        let content = provider.content(huge).expect("the item is a file");
        assert!(matches!(content, ContentSource::Floor { .. }));
        let facts = provider.item(huge).expect("the item exists");
        assert_eq!(facts.size.expect("a file claims a size").bytes, 96 << 30);
    }

    #[test]
    fn a_size_claim_records_what_it_is_a_claim_about() {
        // CP/M states records, not bytes: three records is 384 bytes of
        // allocation and says nothing exact about the file's tail.
        let rounded = SizeClaim {
            bytes: 384,
            basis: SizeBasis::RecordCount {
                record_bytes: 128,
                records: 3,
            },
        };
        assert_eq!(SizeClaim::exact(384).bytes, rounded.bytes);
        assert_ne!(SizeClaim::exact(384).basis, rounded.basis);
    }

    #[test]
    fn a_flat_namespace_is_one_root_of_leaves() {
        let mut provider = SyntheticProvider::new(8);
        let root = provider.add(ItemKind::Container, Vec::new());
        provider.roots.push(root);
        let file = provider.add_file(SizeClaim::exact(4), Vec::new());
        provider.link(root, RecordedName::utf8("A"), file);

        assert_eq!(provider.roots().expect("roots answer").len(), 1);
        let error = provider
            .entries(file)
            .expect_err("a file is not a container");
        assert!(error.to_string().contains("holds no names"), "{error}");
        check_conformance(&provider).expect("the contract holds");
    }

    #[test]
    fn a_refusal_is_attributed_to_the_provider_that_presents() {
        let provider = SyntheticProvider::new(4);
        let error = provider
            .item(ItemRef(3))
            .expect_err("the view holds no item 3");
        assert_eq!(
            error.to_string(),
            format!("invalid {PROVIDER} disk image: this view holds no item 3")
        );
    }
}
