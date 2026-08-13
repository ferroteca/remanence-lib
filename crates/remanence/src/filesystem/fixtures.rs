// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! A synthetic [`FilesystemView`] provider, shared by this module's
//! tests.
//!
//! It presents a small namespace with everything the contract can
//! carry — several names for one item, a recorded name in a declared
//! encoding, a bounded content descriptor, an opaque region — so the
//! contract tests and the coverage tests exercise the same provider
//! rather than two that could drift apart.

use crate::error::Result;
use crate::evidence::Provenance;
use crate::filesystem::contract::*;
use crate::filesystem::coverage::*;

pub(super) const PROVIDER: &str = "test-provider";

pub(super) const OPAQUE_BASE: u64 = 1 << 32;

/// A synthetic system that presents a view of its own structure —
/// the contract's test surface, standing in for a real grammar.
pub(super) struct SyntheticProvider {
    pub(super) floor: Floor,
    /// `(kind, size, hook, entries)` by ref value.
    pub(super) items: Vec<(
        ItemKind,
        Option<SizeClaim>,
        Vec<FloorExtent>,
        Vec<NameEntry>,
    )>,
    pub(super) roots: Vec<ItemRef>,
    pub(super) claims: Vec<(FloorExtent, CoverageClass)>,
}

impl SyntheticProvider {
    pub(super) fn new(total_units: u64) -> Self {
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

    pub(super) fn bytes_floor(total_units: u64) -> Self {
        let mut provider = Self::new(total_units);
        provider.floor = Floor {
            addressing: FloorAddressing::Bytes,
            total_units,
        };
        provider
    }

    pub(super) fn add(&mut self, kind: ItemKind, hook: Vec<FloorExtent>) -> ItemRef {
        self.items.push((kind, None, hook, Vec::new()));
        ItemRef(self.items.len() as u64 - 1)
    }

    pub(super) fn add_file(&mut self, size: SizeClaim, hook: Vec<FloorExtent>) -> ItemRef {
        let item = self.add(ItemKind::File, hook);
        self.items[item.0 as usize].1 = Some(size);
        item
    }

    pub(super) fn link(&mut self, parent: ItemRef, name: RecordedName, target: ItemRef) {
        let ordinal = self.items[parent.0 as usize].3.len() as u64;
        self.items[parent.0 as usize].3.push(NameEntry {
            name,
            target,
            ordinal,
        });
    }

    pub(super) fn claim(&mut self, extent: FloorExtent, class: CoverageClass) {
        self.claims.push((extent, class));
    }

    pub(super) fn entry_names(&self, directory: ItemRef) -> Vec<String> {
        self.entries(directory)
            .expect("the directory exists")
            .into_iter()
            .map(|entry| entry.name.decoded)
            .collect()
    }
}

impl FilesystemView for SyntheticProvider {
    fn source(&self) -> &'static str {
        PROVIDER
    }

    fn floor(&self) -> Floor {
        self.floor
    }

    fn roots(&self) -> Result<Vec<ItemRef>> {
        Ok(self.roots.clone())
    }

    fn entries(&self, directory: ItemRef) -> Result<Vec<NameEntry>> {
        let item = self
            .items
            .get(directory.0 as usize)
            .ok_or_else(|| no_such_item(PROVIDER, directory))?;
        if item.0 != ItemKind::Directory {
            return Err(not_a_directory(PROVIDER, item.0));
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
