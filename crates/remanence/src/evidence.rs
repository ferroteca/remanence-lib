// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The vocabulary for recording what a source said and how the library
//! knows it: declared facts in their own namespace, qualifications noted
//! rather than repaired, and the provenance carried beside both.
//!
//! It belongs to no one layer. A provider at any depth — a file
//! container, a flux capture — states its facts in these terms, so the
//! same reading applies wherever a fact surfaces. Nothing here
//! interprets a fact; that is the declaring namespace's business.
//!
//! The vocabulary is declared ahead of every provider that will speak
//! it, so parts of it have no constructor yet.
#![allow(dead_code)]

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
