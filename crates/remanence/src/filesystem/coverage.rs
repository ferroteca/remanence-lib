// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The coverage account: what every addressable unit of a floor is
//! spent on.
//!
//! **The account is total**, and it is total because the remainder is
//! derived rather than declared — what no claim covers is unclaimed by
//! arithmetic, so a provider cannot leave a hole by forgetting to
//! mention one. Claims that overlap are refused naming both sides, and
//! a claim past the floor is refused in the floor's own units rather
//! than in bytes it never spoke.
//!
//! An **opaque** region is accounted and never named: it is floor spent
//! on something the namespace does not present, which is a different
//! fact from floor that is free. `check_conformance` is the whole of it
//! checked at once.

use crate::error::{Error, Result};
use crate::evidence::Provenance;

use super::*;

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
    /// [`FilesystemView::item`] for one without tracking it itself.
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
pub(crate) fn check_conformance(view: &dyn FilesystemView) -> Result<()> {
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
        if facts.kind != ItemKind::Directory {
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
    use super::super::fixtures::*;
    use super::*;

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
        let root = provider.add(ItemKind::Directory, Vec::new());
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

        impl FilesystemView for ShortAccount {
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
            fn entries(&self, directory: ItemRef) -> Result<Vec<NameEntry>> {
                Err(no_such_item(PROVIDER, directory))
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
}
