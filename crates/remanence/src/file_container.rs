// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The private file-container model behind the P19 seam.
//!
//! This is the one representation every file-bearing provider
//! materializes into: serialized-container catalogs, filesystem
//! adapters, and namespace composers alike. It is not a public
//! interface, an interchange format, or an archive grammar — it holds
//! what a listing *is*, so that no provider keeps a private second
//! notion of it.
//!
//! The model serves both standings P23 distinguishes, and
//! [`Standing`] is where that difference lives. Over a serialized
//! container the model is itself the durable active layer, and there is
//! nothing beneath it to address. Over a filesystem on materialized
//! media it is a derived presentation, items carry footprints in the
//! backing layer's own addressing, and the P19 scope amendment's
//! [`CoverageAccount`] states what the interpretation does and does not
//! claim.
//!
//! Two rules shape everything here:
//!
//! - **Names and items are different things.** The namespace is edges
//!   over an item pool, so one item may carry several names (hard
//!   links), a flat filesystem is one root whose edges all reach
//!   leaves, and hierarchy costs nothing extra. Directory order is
//!   evidence and is preserved as each edge's ordinal; nothing here
//!   re-sorts a source's listing.
//! - **The unclaimed remainder is itemized, never named.** In-force
//!   P19 refuses to manufacture pseudo-files, so an
//!   [`ItemBody::OpaqueRegion`] can never be linked into the
//!   namespace. It is reached through the coverage account instead,
//!   which is what lets a view hold holes without lying about them in
//!   either direction.
//!
//! Metadata follows the two-outcome rule the flux layer established one
//! seam down: a source fact either maps to a named field here, or is
//! retained as a [`DeclaredFact`] under its provider's namespace with
//! its source spelling and order intact. Nothing is normalized on the
//! way in — a timestamp keeps its source precision, epoch, and zone
//! semantics because the provider's namespace says what its value
//! means, and this model never reinterprets it.
//!
//! Nothing is read whole (P27): file content stays behind a bounded
//! [`ContentSource`] descriptor, so a layer over a hundred-gigabyte
//! artifact costs its structure and nothing more.

// The providers that will materialize into this model arrive with their
// own features; until then its constructors have no non-test callers.
#![allow(dead_code)]

use std::fs::File;
use std::sync::Arc;

use crate::error::{Error, Result};

/// A library-owned item identity, stable for the layer's lifetime.
///
/// It is an index into the layer's item pool and means nothing outside
/// the layer that issued it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct ItemId(pub(crate) u32);

/// The encoding a provider claims for a recorded name.
///
/// The set grows additively as providers are admitted, exactly as the
/// flux layer's named homes do: a provider whose encoding has no
/// variant here retains the name's bytes and refuses to claim an
/// encoding it does not implement.
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

/// How faithfully a recorded name's decoded presentation represents its
/// bytes.
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
/// conversion, never in place of them — irregular names (trailing
/// spaces, shift characters, bytes outside the claimed encoding) stay
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

/// One edge of the namespace: a name, in one container, reaching one
/// item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Edge {
    pub(crate) name: RecordedName,
    pub(crate) target: ItemId,
    /// The edge's position in the source's own listing order.
    pub(crate) ordinal: u64,
}

/// What a provider's size claim is actually a claim *about*.
///
/// These are different claims, not one number: an exactly stored byte
/// count, a size expressed in allocation units, and a rounded count of
/// fixed-length records each mean something different about the bytes
/// a read will produce.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SizeBasis {
    /// The source stores the byte length itself.
    Exact,
    /// The source states a count of allocation units only.
    AllocationUnits { unit_bytes: u64, units: u64 },
    /// The source states a count of fixed-length records, so the byte
    /// length is rounded up to the record boundary (CP/M's 128-byte
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
/// This is a descriptor, never the bytes: the model holds no file
/// content resident, and a read resolves it through the owning
/// provider when the caller asks.
pub(crate) enum ContentSource {
    /// A span of the claimed source artifact, read in place.
    InPlace { offset: u64, length: u64 },
    /// Decoded once into private session storage.
    Spooled { spool: Arc<File>, length: u64 },
    /// Assembled from extents of the materialized backing layer, in
    /// the order the provider gives them.
    Backing { extents: Vec<BackingExtent> },
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
            Self::Backing { extents } => f
                .debug_struct("Backing")
                .field("extents", &extents.len())
                .finish(),
            Self::Empty => f.write_str("Empty"),
        }
    }
}

/// What one addressable unit of a materialized backing layer *is*.
///
/// A layer speaks exactly one of these, so footprints and the coverage
/// account share one unit ordering and totality is checkable. The
/// descriptor keeps the vocabulary's own spelling recoverable; the
/// extents below index units in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BackingAddressing {
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

/// A half-open run of addressable units in the backing layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BackingExtent {
    pub(crate) start: u64,
    pub(crate) count: u64,
}

impl BackingExtent {
    pub(crate) const fn new(start: u64, count: u64) -> Self {
        Self { start, count }
    }

    /// The first unit past the extent, or `None` if the run overflows.
    pub(crate) fn end(&self) -> Option<u64> {
        self.start.checked_add(self.count)
    }
}

/// What an item is, and the facts only that kind carries.
///
/// Modeling the kinds this way is what keeps the illegal combinations
/// unrepresentable: only a container owns edges, only a file claims a
/// size and content, and an opaque region can claim neither.
#[derive(Debug)]
pub(crate) enum ItemBody {
    /// A named namespace level. It claims container structure, not disk
    /// allocation.
    Container { edges: Vec<Edge> },
    /// An item with extractable content.
    File {
        size: SizeClaim,
        content: ContentSource,
    },
    /// The itemized remainder of the coverage account: an extent the
    /// interpretation does not claim. It never carries a name, and
    /// interpreting it belongs to whatever lower seam claims it.
    OpaqueRegion,
}

impl ItemBody {
    /// The kind's stable spelling, for diagnostics.
    pub(crate) const fn kind(&self) -> &'static str {
        match self {
            Self::Container { .. } => "container",
            Self::File { .. } => "file",
            Self::OpaqueRegion => "opaque region",
        }
    }
}

/// A fact a provider declares under its own namespace.
///
/// The value means whatever that namespace says it means. The model
/// neither interprets nor normalizes it — which is exactly what lets a
/// timestamp keep its source precision, epoch and zone semantics.
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

/// A source structure retained verbatim because the model has no named
/// home for it yet.
///
/// Retaining it is the first of the two outcomes, not the permanent
/// one: a fact stays foreign only until a later revision gives it a
/// named field, which is what stops this from becoming the model's
/// blind spot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ForeignRecord {
    pub(crate) namespace: &'static str,
    pub(crate) type_id: String,
    pub(crate) version: Option<u32>,
    pub(crate) ordinal: u64,
    /// Where it sat in the source artifact, where the provider can say.
    pub(crate) source_range: Option<BackingExtent>,
    pub(crate) payload: Vec<u8>,
    /// Whatever the provider could safely decode, if anything.
    pub(crate) decoded_summary: Vec<DeclaredFact>,
}

/// Something qualified about an item or the layer, recorded rather than
/// repaired.
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

/// How a fact in this model came to be known.
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

/// One item: its identity, its kind-specific body, and everything the
/// provider claimed about it.
#[derive(Debug)]
pub(crate) struct Item {
    pub(crate) id: ItemId,
    pub(crate) body: ItemBody,
    /// Where the item sits in the materialized backing layer. Always
    /// empty in the active standing, where there is no layer beneath.
    pub(crate) footprint: Vec<BackingExtent>,
    pub(crate) facts: Vec<DeclaredFact>,
    pub(crate) issues: Vec<Issue>,
    pub(crate) provenance: Provenance,
}

impl Item {
    /// The item's edges, or an empty slice for a non-container.
    pub(crate) fn edges(&self) -> &[Edge] {
        match &self.body {
            ItemBody::Container { edges } => edges,
            _ => &[],
        }
    }
}

/// Which P23 standing the layer holds, and so what it can carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Standing {
    /// The file container is itself the durable active layer — a
    /// serialized container such as ZIP. There is no layer beneath it,
    /// so there are no footprints and no coverage account. Source bytes
    /// the grammar does not account for are that adapter's evidence
    /// under P3 and P4, not opaque regions.
    Active,
    /// A derived presentation over a materialized backing layer.
    Derived {
        addressing: BackingAddressing,
        /// The addressable extent the account must cover in full.
        total_units: u64,
    },
}

/// How one addressable unit of the backing is accounted for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CoverageClass {
    /// The data footprint of an item the namespace names.
    ItemData(ItemId),
    /// The namespace's own structures — directory records, allocation
    /// metadata, boot and reserved structures the interpretation
    /// claims. Deleted-but-present directory entries are accounted
    /// here, inside the structures they occupy; itemizing them would be
    /// a recovery claim this model does not make.
    NamespaceStructures,
    /// Space the allocation metadata claims is free. This records that
    /// claim and nothing else: it is not a verdict that the extent is
    /// empty, disposable, or safe to reuse.
    ClaimedFree,
    /// An extent the interpretation does not claim, itemized as the
    /// opaque region it names.
    Opaque(ItemId),
}

/// One classified run of the backing layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CoverageRegion {
    pub(crate) extent: BackingExtent,
    pub(crate) class: CoverageClass,
    /// Why the class was assigned, in human-readable terms (P4).
    pub(crate) evidence: Vec<String>,
}

/// A total, exclusive account of the backing layer's addressable extent.
///
/// Totality is true by construction rather than by assertion: the
/// provider claims what its interpretation covers, and whatever remains
/// becomes opaque regions. Its regions are ordered by position and
/// together span exactly `[0, total_units)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CoverageAccount {
    pub(crate) addressing: BackingAddressing,
    pub(crate) total_units: u64,
    pub(crate) regions: Vec<CoverageRegion>,
}

impl CoverageAccount {
    /// The regions of one class, in position order.
    pub(crate) fn regions_of(
        &self,
        matching: impl Fn(&CoverageClass) -> bool,
    ) -> impl Iterator<Item = &CoverageRegion> {
        self.regions
            .iter()
            .filter(move |region| matching(&region.class))
    }

    /// The number of units accounted to classes matching `matching`.
    pub(crate) fn units_of(&self, matching: impl Fn(&CoverageClass) -> bool) -> u64 {
        self.regions_of(matching)
            .map(|region| region.extent.count)
            .sum()
    }
}

/// Builds a [`CoverageAccount`] by claiming what the interpretation
/// covers and deriving the rest.
///
/// Claims are checked as they arrive (P6): an extent outside the
/// backing, an empty run, or an overlap with an existing claim is
/// refused there and then, naming both sides, rather than producing an
/// account that quietly contradicts itself.
pub(crate) struct CoverageBuilder {
    source: &'static str,
    addressing: BackingAddressing,
    total_units: u64,
    /// Kept ordered by `extent.start`, and disjoint by construction.
    claimed: Vec<CoverageRegion>,
}

impl CoverageBuilder {
    /// Begins an account for `layer`, which must hold the derived
    /// standing — the active standing has no layer beneath to account
    /// for.
    pub(crate) fn for_layer(layer: &FileContainerLayer) -> Result<Self> {
        let Standing::Derived {
            addressing,
            total_units,
        } = layer.standing
        else {
            return Err(layer.refuse(
                "a coverage account needs a materialized backing layer, and this \
                 file container is itself the active layer",
            ));
        };
        Ok(Self {
            source: layer.provenance.source,
            addressing,
            total_units,
            claimed: Vec::new(),
        })
    }

    /// Claims `extent` for `class`, with the evidence for that reading.
    pub(crate) fn claim(
        &mut self,
        extent: BackingExtent,
        class: CoverageClass,
        evidence: Vec<String>,
    ) -> Result<()> {
        if extent.count == 0 {
            return Err(self.refuse(format!(
                "coverage claim at unit {} spans no units; an item that occupies \
                 nothing carries no extent at all",
                extent.start
            )));
        }
        let end = extent.end().ok_or_else(|| {
            self.refuse(format!(
                "coverage claim at unit {} for {} units runs past the end of the \
                 unit space",
                extent.start, extent.count
            ))
        })?;
        if end > self.total_units {
            return Err(self.refuse(format!(
                "coverage claim covers units {}..{end}, past the backing layer's \
                 {} units",
                extent.start, self.total_units
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

    /// Derives the opaque remainder, itemizes it in `layer`, and
    /// installs the finished account.
    ///
    /// Every gap between claims becomes one opaque region item, so the
    /// resulting account spans the backing layer exactly.
    pub(crate) fn finish(self, layer: &mut FileContainerLayer) -> Result<()> {
        for region in &self.claimed {
            if let CoverageClass::ItemData(id) | CoverageClass::Opaque(id) = region.class
                && layer.get(id).is_none()
            {
                return Err(layer.refuse(format!(
                    "coverage claims units {}..{} for item {}, which this layer \
                     does not hold",
                    region.extent.start,
                    region.extent.end().unwrap_or(u64::MAX),
                    id.0
                )));
            }
        }

        let mut regions: Vec<CoverageRegion> = Vec::with_capacity(self.claimed.len() + 1);
        let mut position = 0_u64;
        for region in self.claimed {
            if region.extent.start > position {
                regions.push(
                    layer.itemize_opaque(BackingExtent::new(
                        position,
                        region.extent.start - position,
                    )),
                );
            }
            position = region.extent.end().expect("claims were bounds-checked");
            regions.push(region);
        }
        if position < self.total_units {
            regions.push(
                layer.itemize_opaque(BackingExtent::new(position, self.total_units - position)),
            );
        }

        layer.coverage = Some(CoverageAccount {
            addressing: self.addressing,
            total_units: self.total_units,
            regions,
        });
        Ok(())
    }

    fn overlap(&self, existing: &CoverageRegion, incoming: BackingExtent) -> Error {
        self.refuse(format!(
            "coverage claim covering units {}..{} overlaps the {} already claimed \
             at units {}..{}",
            incoming.start,
            incoming.end().unwrap_or(u64::MAX),
            class_name(&existing.class),
            existing.extent.start,
            existing.extent.end().unwrap_or(u64::MAX),
        ))
    }

    fn refuse(&self, reason: impl Into<String>) -> Error {
        Error::invalid_image(self.source, reason)
    }
}

const fn class_name(class: &CoverageClass) -> &'static str {
    match class {
        CoverageClass::ItemData(_) => "item data",
        CoverageClass::NamespaceStructures => "namespace structures",
        CoverageClass::ClaimedFree => "claimed-free space",
        CoverageClass::Opaque(_) => "opaque region",
    }
}

/// One file container's namespace, items, and — in the derived
/// standing — the account of what it claims over its backing.
#[derive(Debug)]
pub(crate) struct FileContainerLayer {
    standing: Standing,
    /// Each a container item. Several roots are ordinary: a composed
    /// namespace may have one per drive letter or mount.
    roots: Vec<ItemId>,
    /// The item pool, indexed by [`ItemId`].
    items: Vec<Item>,
    coverage: Option<CoverageAccount>,
    /// Container-level metadata, in source order.
    facts: Vec<DeclaredFact>,
    foreign_records: Vec<ForeignRecord>,
    provenance: Provenance,
}

impl FileContainerLayer {
    pub(crate) fn new(standing: Standing, provenance: Provenance) -> Self {
        Self {
            standing,
            roots: Vec::new(),
            items: Vec::new(),
            coverage: None,
            facts: Vec::new(),
            foreign_records: Vec::new(),
            provenance,
        }
    }

    pub(crate) const fn standing(&self) -> Standing {
        self.standing
    }

    pub(crate) fn roots(&self) -> &[ItemId] {
        &self.roots
    }

    pub(crate) fn coverage(&self) -> Option<&CoverageAccount> {
        self.coverage.as_ref()
    }

    pub(crate) fn facts(&self) -> &[DeclaredFact] {
        &self.facts
    }

    pub(crate) fn foreign_records(&self) -> &[ForeignRecord] {
        &self.foreign_records
    }

    pub(crate) fn provenance(&self) -> &Provenance {
        &self.provenance
    }

    /// Every item the layer holds, named or not.
    pub(crate) fn items(&self) -> &[Item] {
        &self.items
    }

    pub(crate) fn get(&self, id: ItemId) -> Option<&Item> {
        self.items.get(id.0 as usize)
    }

    pub(crate) fn get_mut(&mut self, id: ItemId) -> Option<&mut Item> {
        self.items.get_mut(id.0 as usize)
    }

    /// Adds a container item and returns its identity. It is linked
    /// into the namespace by [`Self::link`], or declared a root by
    /// [`Self::add_root`].
    pub(crate) fn add_container(&mut self, provenance: Provenance) -> ItemId {
        self.push(ItemBody::Container { edges: Vec::new() }, provenance)
    }

    /// Adds a file item and returns its identity.
    pub(crate) fn add_file(
        &mut self,
        size: SizeClaim,
        content: ContentSource,
        provenance: Provenance,
    ) -> ItemId {
        self.push(ItemBody::File { size, content }, provenance)
    }

    /// Declares an existing container item a root of the namespace.
    pub(crate) fn add_root(&mut self, id: ItemId) -> Result<()> {
        match self.get(id).map(|item| &item.body) {
            Some(ItemBody::Container { .. }) => {}
            Some(body) => {
                let kind = body.kind();
                return Err(self.refuse(format!("a {kind} cannot be a namespace root")));
            }
            None => return Err(self.no_such_item(id)),
        }
        if !self.roots.contains(&id) {
            self.roots.push(id);
        }
        Ok(())
    }

    /// Names `target` inside the container `parent`.
    ///
    /// Calling this more than once for one target is ordinary — that is
    /// what a hard link is. The edge's ordinal is its position in the
    /// source's own listing order, so the model preserves that order
    /// without ever re-sorting it.
    ///
    /// An opaque region is refused: the namespace lists only what the
    /// source names, and manufacturing an entry for the unclaimed
    /// remainder is the pseudo-file P19 forbids.
    pub(crate) fn link(
        &mut self,
        parent: ItemId,
        name: RecordedName,
        target: ItemId,
    ) -> Result<()> {
        match self.get(target).map(|item| &item.body) {
            Some(ItemBody::OpaqueRegion) => {
                return Err(self.refuse(format!(
                    "the opaque region at item {} cannot be given the name '{}': \
                     the namespace lists only what the source names",
                    target.0, name.decoded
                )));
            }
            Some(_) => {}
            None => return Err(self.no_such_item(target)),
        }
        match self.get(parent).map(|item| &item.body) {
            Some(ItemBody::Container { edges }) => {
                let ordinal = edges.len() as u64;
                let Some(Item {
                    body: ItemBody::Container { edges },
                    ..
                }) = self.items.get_mut(parent.0 as usize)
                else {
                    unreachable!("the parent was just observed to be a container");
                };
                edges.push(Edge {
                    name,
                    target,
                    ordinal,
                });
                Ok(())
            }
            Some(body) => {
                let kind = body.kind();
                Err(self.refuse(format!("a {kind} holds no names")))
            }
            None => Err(self.no_such_item(parent)),
        }
    }

    /// Records `extent` as this item's footprint in the backing layer.
    pub(crate) fn set_footprint(&mut self, id: ItemId, extents: Vec<BackingExtent>) -> Result<()> {
        if matches!(self.standing, Standing::Active) {
            return Err(self.refuse(
                "a footprint needs a materialized backing layer, and this file \
                 container is itself the active layer",
            ));
        }
        match self.items.get_mut(id.0 as usize) {
            Some(item) => {
                item.footprint = extents;
                Ok(())
            }
            None => Err(self.no_such_item(id)),
        }
    }

    pub(crate) fn declare(&mut self, key: impl Into<String>, value: FactValue) {
        let ordinal = self.facts.len() as u64;
        self.facts.push(DeclaredFact {
            namespace: self.provenance.source,
            key: key.into(),
            value,
            ordinal,
        });
    }

    pub(crate) fn retain_foreign(&mut self, record: ForeignRecord) {
        self.foreign_records.push(record);
    }

    /// The items an ordinary namespace walk reaches from the roots.
    ///
    /// An item with no edge to it — the opaque remainder — is
    /// unreachable here by construction, which is the whole point: it
    /// is reached through the coverage account instead.
    pub(crate) fn reachable(&self) -> Vec<ItemId> {
        let mut seen = vec![false; self.items.len()];
        let mut order = Vec::new();
        let mut pending: Vec<ItemId> = self.roots.iter().rev().copied().collect();
        while let Some(id) = pending.pop() {
            let index = id.0 as usize;
            if seen.get(index).copied().unwrap_or(true) {
                continue;
            }
            seen[index] = true;
            order.push(id);
            let Some(item) = self.items.get(index) else {
                continue;
            };
            for edge in item.edges().iter().rev() {
                pending.push(edge.target);
            }
        }
        order
    }

    fn push(&mut self, body: ItemBody, provenance: Provenance) -> ItemId {
        let id = ItemId(self.items.len() as u32);
        self.items.push(Item {
            id,
            body,
            footprint: Vec::new(),
            facts: Vec::new(),
            issues: Vec::new(),
            provenance,
        });
        id
    }

    /// Adds the unnamed item standing for one unclaimed extent, and
    /// returns the coverage region naming it.
    fn itemize_opaque(&mut self, extent: BackingExtent) -> CoverageRegion {
        let id = self.push(
            ItemBody::OpaqueRegion,
            Provenance::new(self.provenance.source)
                .note("the interpretation claims no structure over this extent"),
        );
        self.items[id.0 as usize].footprint = vec![extent];
        CoverageRegion {
            extent,
            class: CoverageClass::Opaque(id),
            evidence: vec![format!(
                "units {}..{} are covered by no claim of this interpretation",
                extent.start,
                extent.end().unwrap_or(u64::MAX)
            )],
        }
    }

    fn no_such_item(&self, id: ItemId) -> Error {
        self.refuse(format!("this layer holds no item {}", id.0))
    }

    fn refuse(&self, reason: impl Into<String>) -> Error {
        Error::invalid_image(self.provenance.source, reason)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCategory;

    const PROVIDER: &str = "test-provider";

    fn derived_layer(total_units: u64) -> FileContainerLayer {
        FileContainerLayer::new(
            Standing::Derived {
                addressing: BackingAddressing::Blocks {
                    bytes_per_block: 256,
                },
                total_units,
            },
            Provenance::new(PROVIDER),
        )
    }

    fn active_layer() -> FileContainerLayer {
        FileContainerLayer::new(Standing::Active, Provenance::new(PROVIDER))
    }

    fn named(layer: &FileContainerLayer, parent: ItemId) -> Vec<String> {
        layer
            .get(parent)
            .expect("the parent exists")
            .edges()
            .iter()
            .map(|edge| edge.name.decoded.clone())
            .collect()
    }

    #[test]
    fn one_item_may_carry_several_names() {
        let mut layer = active_layer();
        let root = layer.add_container(Provenance::new(PROVIDER));
        layer.add_root(root).expect("a container may be a root");
        let file = layer.add_file(
            SizeClaim::exact(12),
            ContentSource::InPlace {
                offset: 0,
                length: 12,
            },
            Provenance::new(PROVIDER),
        );

        layer
            .link(root, RecordedName::utf8("README"), file)
            .expect("the first name links");
        layer
            .link(root, RecordedName::utf8("readme.txt"), file)
            .expect("a hard link is ordinary");

        let edges = layer.get(root).expect("the root exists").edges();
        assert_eq!(edges.len(), 2);
        assert_eq!(edges[0].target, edges[1].target);
        // One item, two names: the pool did not grow with the namespace.
        assert_eq!(layer.items().len(), 2);
        assert_eq!(layer.reachable().len(), 2);
    }

    #[test]
    fn source_listing_order_is_preserved_as_evidence() {
        let mut layer = active_layer();
        let root = layer.add_container(Provenance::new(PROVIDER));
        layer.add_root(root).expect("a container may be a root");
        for name in ["ZEBRA", "alpha", "Middle"] {
            let file = layer.add_file(
                SizeClaim::exact(0),
                ContentSource::Empty,
                Provenance::new(PROVIDER),
            );
            layer
                .link(root, RecordedName::utf8(name), file)
                .expect("the name links");
        }

        assert_eq!(named(&layer, root), ["ZEBRA", "alpha", "Middle"]);
        let ordinals: Vec<u64> = layer
            .get(root)
            .expect("the root exists")
            .edges()
            .iter()
            .map(|edge| edge.ordinal)
            .collect();
        assert_eq!(ordinals, [0, 1, 2]);
    }

    #[test]
    fn a_name_keeps_its_recorded_bytes_and_claimed_encoding() {
        let mut layer = active_layer();
        let root = layer.add_container(Provenance::new(PROVIDER));
        let file = layer.add_file(
            SizeClaim::exact(2),
            ContentSource::Empty,
            Provenance::new(PROVIDER),
        );
        // A CBM name padded with shifted spaces: the bytes are the
        // evidence, and the presentation never replaces them.
        let recorded = RecordedName::new(
            vec![0x50, 0x49, 0x43, 0xa0, 0xa0],
            NameEncoding::Petscii,
            "PIC",
            NameConversion::Lossy {
                detail: "two trailing shifted spaces are not presentable".to_owned(),
            },
        );
        layer.link(root, recorded, file).expect("the name links");

        let edge = &layer.get(root).expect("the root exists").edges()[0];
        assert_eq!(edge.name.bytes, [0x50, 0x49, 0x43, 0xa0, 0xa0]);
        assert_eq!(edge.name.encoding, NameEncoding::Petscii);
        assert_eq!(edge.name.decoded, "PIC");
        assert!(matches!(edge.name.conversion, NameConversion::Lossy { .. }));
    }

    #[test]
    fn declared_facts_keep_their_source_spelling_and_order() {
        let mut layer = active_layer();
        layer.declare("created", FactValue::Unsigned(0x2f_1a_63_00));
        layer.declare("Comment", FactValue::Text("as recorded".to_owned()));

        let facts = layer.facts();
        assert_eq!(facts[0].key, "created");
        assert_eq!(facts[0].ordinal, 0);
        assert_eq!(facts[1].key, "Comment");
        assert_eq!(facts[1].ordinal, 1);
        assert!(facts.iter().all(|fact| fact.namespace == PROVIDER));
    }

    #[test]
    fn an_opaque_region_can_never_be_given_a_name() {
        let mut layer = derived_layer(16);
        let root = layer.add_container(Provenance::new(PROVIDER));
        layer.add_root(root).expect("a container may be a root");

        let mut coverage = CoverageBuilder::for_layer(&layer).expect("the layer is derived");
        coverage
            .claim(
                BackingExtent::new(0, 4),
                CoverageClass::NamespaceStructures,
                vec!["the directory occupies the first four blocks".to_owned()],
            )
            .expect("the claim is in bounds");
        coverage.finish(&mut layer).expect("the account completes");

        let opaque = layer
            .coverage()
            .expect("the account is installed")
            .regions
            .iter()
            .find_map(|region| match region.class {
                CoverageClass::Opaque(id) => Some(id),
                _ => None,
            })
            .expect("the remainder was itemized");

        let error = layer
            .link(root, RecordedName::utf8("PROTECTION"), opaque)
            .expect_err("the pseudo-file rule holds");
        assert_eq!(error.category(), ErrorCategory::InvalidImage);
        assert!(
            error
                .to_string()
                .contains("lists only what the source names"),
            "{error}"
        );
        // And it stays out of the walk that a caller would list.
        assert!(!layer.reachable().contains(&opaque));
    }

    #[test]
    fn coverage_is_total_because_the_remainder_is_derived() {
        let mut layer = derived_layer(100);
        let file = layer.add_file(
            SizeClaim::exact(2560),
            ContentSource::Backing {
                extents: vec![BackingExtent::new(10, 10)],
            },
            Provenance::new(PROVIDER),
        );

        let mut coverage = CoverageBuilder::for_layer(&layer).expect("the layer is derived");
        coverage
            .claim(
                BackingExtent::new(0, 4),
                CoverageClass::NamespaceStructures,
                Vec::new(),
            )
            .expect("in bounds");
        coverage
            .claim(
                BackingExtent::new(10, 10),
                CoverageClass::ItemData(file),
                Vec::new(),
            )
            .expect("in bounds");
        coverage
            .claim(
                BackingExtent::new(20, 60),
                CoverageClass::ClaimedFree,
                Vec::new(),
            )
            .expect("in bounds");
        coverage.finish(&mut layer).expect("the account completes");

        let account = layer.coverage().expect("the account is installed");
        // Every unit is accounted for exactly once, and the regions run
        // in position order without a gap.
        let mut position = 0;
        for region in &account.regions {
            assert_eq!(region.extent.start, position);
            position = region.extent.end().expect("bounded");
        }
        assert_eq!(position, 100);
        assert_eq!(
            account.units_of(|class| matches!(class, CoverageClass::Opaque(_))),
            26,
            "units 4..10 and 80..100 belong to no claim"
        );
        assert_eq!(
            account.units_of(|class| matches!(class, CoverageClass::ClaimedFree)),
            60
        );
    }

    #[test]
    fn claimed_free_and_opaque_are_different_answers() {
        let mut layer = derived_layer(10);
        let mut coverage = CoverageBuilder::for_layer(&layer).expect("the layer is derived");
        coverage
            .claim(
                BackingExtent::new(0, 5),
                CoverageClass::ClaimedFree,
                Vec::new(),
            )
            .expect("in bounds");
        coverage.finish(&mut layer).expect("the account completes");

        let account = layer.coverage().expect("the account is installed");
        assert_eq!(account.regions.len(), 2);
        assert!(matches!(
            account.regions[0].class,
            CoverageClass::ClaimedFree
        ));
        assert!(matches!(account.regions[1].class, CoverageClass::Opaque(_)));
    }

    #[test]
    fn overlapping_claims_are_refused_naming_both_sides() {
        let layer = derived_layer(50);
        let mut coverage = CoverageBuilder::for_layer(&layer).expect("the layer is derived");
        coverage
            .claim(
                BackingExtent::new(10, 10),
                CoverageClass::NamespaceStructures,
                Vec::new(),
            )
            .expect("in bounds");

        let error = coverage
            .claim(
                BackingExtent::new(15, 10),
                CoverageClass::ClaimedFree,
                Vec::new(),
            )
            .expect_err("the extents overlap");
        assert_eq!(error.category(), ErrorCategory::InvalidImage);
        let message = error.to_string();
        assert!(message.contains("15..25"), "{message}");
        assert!(message.contains("namespace structures"), "{message}");
        assert!(message.contains("10..20"), "{message}");
    }

    #[test]
    fn a_claim_past_the_backing_layer_is_refused() {
        let layer = derived_layer(32);
        let mut coverage = CoverageBuilder::for_layer(&layer).expect("the layer is derived");
        let error = coverage
            .claim(
                BackingExtent::new(30, 8),
                CoverageClass::ClaimedFree,
                Vec::new(),
            )
            .expect_err("the claim runs past the end");
        assert!(
            error
                .to_string()
                .contains("past the backing layer's 32 units")
        );

        assert!(
            coverage
                .claim(
                    BackingExtent::new(0, 0),
                    CoverageClass::ClaimedFree,
                    Vec::new()
                )
                .is_err(),
            "an extent spanning no units claims nothing"
        );
    }

    #[test]
    fn coverage_referring_to_an_absent_item_is_refused() {
        let mut layer = derived_layer(20);
        let mut coverage = CoverageBuilder::for_layer(&layer).expect("the layer is derived");
        coverage
            .claim(
                BackingExtent::new(0, 4),
                CoverageClass::ItemData(ItemId(7)),
                Vec::new(),
            )
            .expect("bounds are checked before identity");

        let error = coverage
            .finish(&mut layer)
            .expect_err("item 7 does not exist");
        assert!(error.to_string().contains("does not hold"), "{error}");
        assert!(layer.coverage().is_none(), "no account was installed");
    }

    #[test]
    fn the_active_standing_has_no_backing_to_account_for() {
        let mut layer = active_layer();
        let file = layer.add_file(
            SizeClaim::exact(1),
            ContentSource::Empty,
            Provenance::new(PROVIDER),
        );

        let error = CoverageBuilder::for_layer(&layer)
            .err()
            .expect("there is no layer beneath");
        assert!(
            error.to_string().contains("itself the active layer"),
            "{error}"
        );

        let error = layer
            .set_footprint(file, vec![BackingExtent::new(0, 1)])
            .expect_err("a footprint needs a backing layer");
        assert!(
            error.to_string().contains("itself the active layer"),
            "{error}"
        );
    }

    #[test]
    fn content_stays_a_bounded_descriptor() {
        let mut layer = active_layer();
        let huge = layer.add_file(
            SizeClaim {
                bytes: 96 * 1024 * 1024 * 1024,
                basis: SizeBasis::AllocationUnits {
                    unit_bytes: 4096,
                    units: 25_165_824,
                },
            },
            ContentSource::InPlace {
                offset: 512,
                length: 96 * 1024 * 1024 * 1024,
            },
            Provenance::new(PROVIDER),
        );

        // The layer describes ninety-six gibibytes without holding any
        // of it: the size is a claim and the content is a descriptor.
        let ItemBody::File { size, content } = &layer.get(huge).expect("the item exists").body
        else {
            panic!("the item is a file");
        };
        assert_eq!(size.bytes, 96 * 1024 * 1024 * 1024);
        assert!(matches!(
            content,
            ContentSource::InPlace { offset: 512, .. }
        ));
    }

    #[test]
    fn a_size_claim_records_what_it_is_a_claim_about() {
        // CP/M states records, not bytes: 3 records is 384 bytes of
        // allocation and says nothing exact about the file's tail.
        let rounded = SizeClaim {
            bytes: 384,
            basis: SizeBasis::RecordCount {
                record_bytes: 128,
                records: 3,
            },
        };
        assert_ne!(rounded.basis, SizeBasis::Exact);
        assert_eq!(SizeClaim::exact(384).bytes, rounded.bytes);
        assert_ne!(SizeClaim::exact(384).basis, rounded.basis);
    }

    #[test]
    fn a_flat_namespace_is_one_root_of_leaves() {
        let mut layer = derived_layer(8);
        let root = layer.add_container(Provenance::new(PROVIDER));
        layer.add_root(root).expect("a container may be a root");
        let file = layer.add_file(
            SizeClaim::exact(4),
            ContentSource::Empty,
            Provenance::new(PROVIDER),
        );
        layer
            .link(root, RecordedName::utf8("A"), file)
            .expect("the name links");

        assert_eq!(layer.roots().len(), 1);
        assert_eq!(layer.reachable(), vec![root, file]);
        assert!(layer.get(file).expect("the file exists").edges().is_empty());
    }

    #[test]
    fn only_a_container_holds_names_or_roots_the_namespace() {
        let mut layer = active_layer();
        let file = layer.add_file(
            SizeClaim::exact(0),
            ContentSource::Empty,
            Provenance::new(PROVIDER),
        );
        let other = layer.add_file(
            SizeClaim::exact(0),
            ContentSource::Empty,
            Provenance::new(PROVIDER),
        );

        let error = layer
            .link(file, RecordedName::utf8("child"), other)
            .expect_err("a file holds no names");
        assert!(error.to_string().contains("holds no names"), "{error}");

        let error = layer
            .add_root(file)
            .expect_err("a file cannot root a namespace");
        assert!(
            error.to_string().contains("cannot be a namespace root"),
            "{error}"
        );
    }

    #[test]
    fn a_refusal_is_attributed_to_the_provider_that_claimed_the_layer() {
        let mut layer = active_layer();
        let error = layer
            .add_root(ItemId(3))
            .expect_err("the layer holds no item 3");
        assert_eq!(
            error.to_string(),
            format!("invalid {PROVIDER} disk image: this layer holds no item 3")
        );
    }

    #[test]
    fn foreign_records_are_retained_in_order() {
        let mut layer = active_layer();
        for (ordinal, type_id) in ["0x5455", "0x7875"].into_iter().enumerate() {
            layer.retain_foreign(ForeignRecord {
                namespace: PROVIDER,
                type_id: type_id.to_owned(),
                version: None,
                ordinal: ordinal as u64,
                source_range: None,
                payload: vec![0xde, 0xad],
                decoded_summary: Vec::new(),
            });
        }

        let records = layer.foreign_records();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].type_id, "0x5455");
        assert_eq!(records[1].ordinal, 1);
        assert_eq!(records[1].payload, [0xde, 0xad]);
    }
}
