// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The namespace node — [`StorageSpace`], the one type that carries file
//! verbs — and the P19 presentation contract beneath it.
//!
//! **File access lives here and nowhere else.** A medium bearing a
//! partition table and a `get_file` would be a category error in the
//! type rather than a refusal waiting to happen, so the medium exposes
//! no file access at all. The space is reached through the partition
//! that composes it: the vantage doors on
//! [`PartitionView`](crate::PartitionView) hand out this node, one door
//! per vantage and the same node behind both.
//!
//! A [`StorageSpace`] is a **view over its provider's state, never an
//! instance** (P23): its mutations project into the active layer, and it
//! stops answering when the medium beneath it leaves. Its kind — FAT,
//! HDOS — is data on the handle, not a type of its own.
//!
//! The rest of this module is the interface every file-bearing provider
//! presents through at the P19 seam, and the vocabulary they answer in.
//!
//! **There is no namespace layer here.** P19 makes the namespace a seam:
//! adapters *expose* a view and results *present* an interface. A ZIP
//! grammar, a FAT volume, a Commodore directory and a composed namespace
//! are real systems that already hold their own structure, and each of
//! them can present a view of it. Nothing in this module materializes a
//! namespace above them — there is no intermediate representation to
//! copy into, and so no second place a listing lives and nothing to
//! invalidate when the truth beneath changes.
//!
//! That is what keeps reads bounded (P27): a provider answers for the
//! directory it was asked about by reading that directory, rather than
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
//!   leaves, and hierarchy is a directory whose entries reach
//!   directories. Listing order is evidence, carried as each entry's
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
//! means. [`EntryFact`] is that rule at the node's own surface.
//!
//! [`entries`]: FilesystemView::entries

// The providers that will present through the P19 contract below arrive
// with their own features; until then it has no non-test implementors.
#![allow(dead_code)]

// The recognizers that present through the contract above: `catalog` the
// streamed adapters for a medium's own namespace, and one module per
// filesystem beneath it. `dos_name` and `dos_letters` are FAT's two name
// seams — the 8.3 decisions one volume makes, and the P19 mapping a DOS
// machine's whole device set derives.
pub(crate) mod catalog;
pub(crate) mod cbm_dos;
pub(crate) mod dos_letters;
pub(crate) mod dos_name;
pub(crate) mod fat;
pub(crate) mod hdos;

pub(crate) mod contract;
pub(crate) mod coverage;
pub(crate) mod space;

#[cfg(test)]
mod fixtures;

pub(crate) use contract::*;
pub(crate) use coverage::*;
pub use space::{File, StorageSpace};
pub(crate) use space::{SpaceExtent, SpaceNamespace};

use crate::error::{Error, ErrorCategory, Result};
use crate::filesystem::fat::{FatEntry, FatEntryKind};
use crate::model::report::VolumeLabel;

/// Which rule of the storage-space seam's enumerated set a refusal broke
/// (P10).
///
/// The set answers *which rule did this input break* where the category
/// answers *how should the caller behave*, and it belongs to this seam
/// rather than to a second library-wide enum — [`crate::DosNameRule`] is
/// the same shape at the DOS 8.3 namespace, and
/// [`crate::PartitionRule`] the same shape one vantage out.
///
/// A [`StorageSpace`] carries two vantages, so the set covers both: the
/// absences are what a space answers when asked for a vantage it does not
/// have, which is trait presence stated as a refusal rather than a
/// failure to read something.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpaceRule {
    /// Nothing beneath this space bears a namespace at all. This is the
    /// honest absence P19 requires, not a failure to read one.
    NoNamespace,
    /// A namespace this release recognizes and does not read.
    RecognizedNotRead,
    /// A namespace this release reads and does not write.
    NotWritable,
    /// The space has no addressable vantage: it is a namespace composed
    /// over something that is not one addressed extent, so there is no
    /// position to read or write by. An archive's namespace is the case
    /// this release reaches.
    NotAddressable,
    /// A read or write named a position outside the space's own extent.
    /// The bound is the space's, not the medium's, which is the point of
    /// addressing within one.
    OutsideExtent,
}

impl SpaceRule {
    /// Every rule in the set.
    pub const ALL: [Self; 5] = [
        Self::NoNamespace,
        Self::RecognizedNotRead,
        Self::NotWritable,
        Self::NotAddressable,
        Self::OutsideExtent,
    ];

    /// The stable cross-language spelling of this rule, which is what a
    /// refusal carries.
    pub const fn as_str(self) -> crate::error::RuleIdentity {
        match self {
            Self::NoNamespace => "no-namespace",
            Self::RecognizedNotRead => "recognized-not-read",
            Self::NotWritable => "namespace-not-writable",
            Self::NotAddressable => "not-addressable",
            Self::OutsideExtent => "outside-extent",
        }
    }

    /// Reads a rule identity back into this set, for a caller branching
    /// on [`Error::rule`](crate::Error::rule). An identity from another
    /// seam's set is `None` rather than a nearest match.
    pub fn from_identity(identity: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|rule| rule.as_str() == identity)
    }
}

impl std::fmt::Display for SpaceRule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

fn refuse(rule: SpaceRule, reason: impl Into<String>) -> Error {
    let category = match rule {
        SpaceRule::RecognizedNotRead | SpaceRule::NotAddressable => ErrorCategory::Unsupported,
        SpaceRule::NoNamespace => ErrorCategory::NotFound,
        SpaceRule::NotWritable => ErrorCategory::ReadOnly,
        SpaceRule::OutsideExtent => ErrorCategory::Io,
    };
    Error::categorized_io(category, reason).broke_rule(rule.as_str())
}

/// What an entry names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    File,
    Directory,
}

impl EntryKind {
    /// The stable cross-language spelling of this kind.
    pub const fn name(self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Directory => "directory",
        }
    }
}

/// A fact the recognizing filesystem declares about an entry that this
/// vocabulary has no named field for, in that filesystem's own spelling.
///
/// This is the two-outcome rule at the node's surface: a source fact
/// either maps to a named field of [`Entry`], or is retained here with
/// its source spelling intact. Nothing is normalized on the way through,
/// so an HDOS catalog date keeps HDOS's reading of it and no caller has
/// to know which epoch it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryFact {
    /// The key as the recognizing filesystem spells it.
    pub key: String,
    /// The value as that filesystem reads it.
    pub value: String,
}

impl EntryFact {
    pub(crate) fn new(key: &str, value: impl Into<String>) -> Self {
        Self {
            key: key.to_owned(),
            value: value.into(),
        }
    }
}

/// One entry of a namespace, in the recognizing filesystem's own terms.
///
/// The name is as stored — nothing is truncated, transliterated, or
/// renamed to fit — and the listing keeps the source's own order, which
/// is evidence (U4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub kind: EntryKind,
    pub size_bytes: u64,
    /// What the recognizing filesystem declares beyond the fields above,
    /// in its own spelling and order. A filesystem whose format records
    /// none declares none.
    pub declared: Vec<EntryFact>,
}

impl Entry {
    /// One declared fact, by the key the recognizing filesystem spells
    /// it with — `fact("size-blocks")` on a CBM DOS entry, because CBM
    /// records size in blocks. `None` where the filesystem declared no
    /// such fact, absence being an answer.
    pub fn fact(&self, key: &str) -> Option<&str> {
        self.declared
            .iter()
            .find(|fact| fact.key == key)
            .map(|fact| fact.value.as_str())
    }

    fn from_fat(entry: &FatEntry) -> Self {
        Self {
            name: entry.name.clone(),
            kind: match entry.kind {
                FatEntryKind::File => EntryKind::File,
                FatEntryKind::Directory => EntryKind::Directory,
            },
            size_bytes: entry.size_bytes,
            declared: Vec::new(),
        }
    }
}

/// A namespace a medium bears directly, materialized bounded by the
/// adapter that recognized it.
///
/// A volume-backed filesystem needs none of this — it reads through the
/// composed volume — so this is the seam for the media whose namespace
/// *is* their content.
pub(crate) trait Catalog: std::fmt::Debug {
    fn entries(&self, path: &str) -> Result<Vec<Entry>>;
    fn stat(&self, path: &str) -> Result<Option<Entry>>;
    fn read_file(&self, path: &str) -> Result<Vec<u8>>;

    /// The label this namespace's own structures carry, where its format
    /// has such a field at all. A grammar that names no volume answers
    /// `None`, which is a different fact from a field that is present
    /// and blank.
    fn label(&self) -> Option<VolumeLabel> {
        None
    }

    /// What recognized this namespace, in human-readable terms (P4).
    ///
    /// A grammar recognized by the load that produced it — an archive's,
    /// which was claimed before there was a namespace to speak of —
    /// states nothing here, and the empty account is that fact rather
    /// than an omission.
    fn evidence(&self) -> Vec<String> {
        Vec::new()
    }

    /// Exactly `buf.len()` bytes at `offset` within one item (P27).
    ///
    /// The default is the honest one for a catalog whose medium is
    /// already resident within its declared bound — a slice of what is
    /// there, rather than a second pass. A provider whose items are
    /// spans of something larger overrides it and reads the span, so
    /// nothing is materialized to serve part of an item.
    fn read_file_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<()> {
        let bytes = self.read_file(path)?;
        let end = offset
            .checked_add(buf.len() as u64)
            .filter(|end| *end <= bytes.len() as u64)
            .ok_or_else(|| {
                Error::categorized_io(
                    ErrorCategory::NotFound,
                    format!(
                        "'{path}' holds {} bytes; the requested span at \
                         {offset} runs past it",
                        bytes.len()
                    ),
                )
            })?;
        buf.copy_from_slice(&bytes[offset as usize..end as usize]);
        Ok(())
    }
}
