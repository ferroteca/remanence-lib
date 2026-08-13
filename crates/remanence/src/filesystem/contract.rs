// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The P19 presentation contract every filesystem adapter presents
//! through.
//!
//! A provider states what it recorded, in its own spelling, and this is
//! the vocabulary it states it in: a [`RecordedName`] keeps the bytes as
//! written beside the encoding claimed for them, an [`ItemRef`] is the
//! provider's own identity for an item so that several names may reach
//! one, a [`SizeClaim`] records what the size is a claim *about*, and a
//! [`ContentSource`] stays a bounded descriptor rather than bytes.
//! [`ForeignRecord`] is the same refusal the capture layer makes: a
//! record this layer cannot name is kept whole rather than dropped.
//!
//! [`FilesystemView`] is the trait itself, and the three refusals below
//! are attributed to the provider that presents rather than to this
//! seam.

use std::fs::File as SpoolFile;
use std::sync::Arc;

use crate::error::{Error, Result};
use crate::evidence::{DeclaredFact, Issue, Provenance};

use super::*;

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

/// One name in one directory, reaching one item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NameEntry {
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
    Directory,
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
            Self::Directory => "directory",
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
    Spooled { spool: Arc<SpoolFile>, length: u64 },
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
/// [`account`]: FilesystemView::account
pub(crate) trait FilesystemView {
    /// The provider namespace, which attributes its refusals.
    fn source(&self) -> &'static str;

    /// The floor this is a view of.
    fn floor(&self) -> Floor;

    /// The namespace's roots, each a directory. Several are ordinary:
    /// a composed namespace may have one per drive letter or mount.
    fn roots(&self) -> Result<Vec<ItemRef>>;

    /// The entries of one directory, in the source's own order.
    ///
    /// An entry never targets an opaque region: the namespace lists
    /// only what the source names.
    fn entries(&self, directory: ItemRef) -> Result<Vec<NameEntry>>;

    /// What the provider claims about one item.
    fn item(&self, item: ItemRef) -> Result<ItemFacts>;

    /// Where one file item's bytes come from, bounded.
    fn content(&self, item: ItemRef) -> Result<ContentSource>;

    /// The total account of the floor, computed on request.
    fn account(&self) -> Result<CoverageAccount>;
}

/// The refusal for an operation that needs a directory.
pub(crate) fn not_a_directory(source: &'static str, kind: ItemKind) -> Error {
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

#[cfg(test)]
mod tests {
    use super::super::fixtures::*;
    use super::*;

    #[test]
    fn one_item_may_be_reached_by_several_names() {
        let mut provider = SyntheticProvider::new(16);
        let root = provider.add(ItemKind::Directory, Vec::new());
        provider.roots.push(root);
        let file = provider.add_file(SizeClaim::exact(12), vec![FloorExtent::new(4, 1)]);

        provider.link(root, RecordedName::utf8("README"), file);
        provider.link(root, RecordedName::utf8("readme.txt"), file);

        let entries = provider.entries(root).expect("the root is a directory");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].target, entries[1].target);
        check_conformance(&provider).expect("the contract holds");
    }

    #[test]
    fn source_listing_order_is_preserved_as_evidence() {
        let mut provider = SyntheticProvider::new(16);
        let root = provider.add(ItemKind::Directory, Vec::new());
        provider.roots.push(root);
        for name in ["ZEBRA", "alpha", "Middle"] {
            let file = provider.add_file(SizeClaim::exact(0), Vec::new());
            provider.link(root, RecordedName::utf8(name), file);
        }

        assert_eq!(provider.entry_names(root), ["ZEBRA", "alpha", "Middle"]);
        let ordinals: Vec<u64> = provider
            .entries(root)
            .expect("the root is a directory")
            .iter()
            .map(|entry| entry.ordinal)
            .collect();
        assert_eq!(ordinals, [0, 1, 2]);
    }

    #[test]
    fn a_name_keeps_its_recorded_bytes_and_claimed_encoding() {
        let mut provider = SyntheticProvider::new(16);
        let root = provider.add(ItemKind::Directory, Vec::new());
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

        let entries = provider.entries(root).expect("the root is a directory");
        let name = &entries[0].name;
        assert_eq!(name.bytes, [0x50, 0x49, 0x43, 0xa0, 0xa0]);
        assert_eq!(name.encoding, NameEncoding::Petscii);
        assert_eq!(name.decoded, "PIC");
        assert!(matches!(name.conversion, NameConversion::Lossy { .. }));
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
        let root = provider.add(ItemKind::Directory, Vec::new());
        provider.roots.push(root);
        let file = provider.add_file(SizeClaim::exact(4), Vec::new());
        provider.link(root, RecordedName::utf8("A"), file);

        assert_eq!(provider.roots().expect("roots answer").len(), 1);
        let error = provider
            .entries(file)
            .expect_err("a file is not a directory");
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
