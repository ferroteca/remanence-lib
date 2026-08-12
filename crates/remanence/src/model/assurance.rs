// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The assurance seam (P28): what one open established about the evidence
//! beneath it, and what that narrows.
//!
//! Fail-closed is a rule about authority, not a command to discard every
//! byte whose complete intended interpretation cannot be proved. Every
//! open therefore carries one explicit outcome — verified, degraded, or
//! refused — decided by a deterministic gate rather than by a second score
//! beside P4's recognition confidence: are every fact and every bound the
//! requested interpretation needs available and coherent?
//!
//! A degraded session keeps the evidence it does have. It states the
//! declaration, what the source actually holds, the first byte that is not
//! there, and the exact extent that reads, and it withholds mutation
//! authority for the session's whole life. Reads inside the extent answer
//! normally; an operation that would need what is missing is refused by
//! name, never clipped, zero-filled, or shortened.
//!
//! The gate is deliberately narrow. It belongs to determining a catalog
//! type and to reading or writing through one, and to nothing else: a
//! failed claim, a cache or private-session-storage failure, a journal
//! operation, an allocation, or host I/O stops immediately under P6 and is
//! never re-described as imperfect media evidence.

use std::fmt;

use crate::error::{Error, ErrorCategory, Result, RuleIdentity};
use crate::io::device::{AccessMode, Claim};

/// What one open established about the evidence beneath it (P28).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssuranceOutcome {
    /// The selected interpretation and every bound the requested
    /// operations need are evidenced.
    Verified,
    /// A material shortfall is known, and a truthful read-only
    /// interpretation of a bounded portion remains.
    Degraded,
    /// No bounded interpretation exists, or an operation needs the
    /// missing or contradictory fact. This outcome is delivered as a
    /// refusal — the ordinary error path, carrying the same condition —
    /// so it names a state no open session is ever in.
    Refused,
}

impl AssuranceOutcome {
    /// The stable cross-language spelling of this outcome.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Verified => "verified",
            Self::Degraded => "degraded",
            Self::Refused => "refused",
        }
    }
}

impl fmt::Display for AssuranceOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Which shortfall narrowed a session's authority, as an enumerated claim
/// (P3) owned by this seam.
///
/// This is the rule set a degraded session's refusals draw their rule
/// identity from (P10): the category still says how a caller should
/// behave, and this says which condition withheld the operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssuranceCondition {
    /// The interpretation declares more bytes than the source holds.
    SourceTruncated,
    /// Required structure contradicts itself, so no safe bound can be
    /// stated for the shortfall observed beside it.
    EvidenceConflict,
}

impl AssuranceCondition {
    /// Every condition in the set.
    pub const ALL: [Self; 2] = [Self::SourceTruncated, Self::EvidenceConflict];

    /// The stable cross-language spelling of this condition, which is what
    /// a withheld operation's refusal carries.
    pub const fn as_str(self) -> RuleIdentity {
        match self {
            Self::SourceTruncated => "source-truncated",
            Self::EvidenceConflict => "evidence-conflict",
        }
    }

    /// Reads a rule identity back into this set, for a caller branching on
    /// [`Error::rule`](crate::Error::rule). An identity from another
    /// seam's set — or from a later revision of this one — is `None`
    /// rather than a nearest match.
    pub fn from_identity(identity: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|condition| condition.as_str() == identity)
    }
}

impl fmt::Display for AssuranceCondition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A half-open byte range, in the addressing of the thing it bounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ByteRange {
    pub start: u64,
    pub end: u64,
}

impl ByteRange {
    pub const fn new(start: u64, end: u64) -> Self {
        Self { start, end }
    }

    /// How many bytes the range covers.
    pub const fn len(&self) -> u64 {
        self.end.saturating_sub(self.start)
    }

    pub const fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl fmt::Display for ByteRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}..{}", self.start, self.end)
    }
}

/// The assurance state one open established (P28), available before
/// anything is read from it.
///
/// A verified session states the outcome and the whole extent as readable;
/// a degraded one adds the condition, the ordered evidence for it, the
/// declared and observed sizes, the first byte that is not there, and the
/// effective access mode its evidence permits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assurance {
    /// What the open established.
    pub outcome: AssuranceOutcome,
    /// Which shortfall narrowed the session, where one did.
    pub condition: Option<AssuranceCondition>,
    /// Why, in the order the observations were made (P4).
    pub evidence: Vec<String>,
    /// The exact extents of the medium that read.
    pub readable: Vec<ByteRange>,
    /// The access this session actually has, which for a degraded session
    /// is read-only whatever intent the caller declared.
    pub access: AccessMode,
    /// Whose open this medium's P7 claim is — the library's own denial,
    /// or the caller's handle honoured as it was afforded.
    pub claim: Claim,
    /// The size the interpretation declares, where one declares a size.
    pub declared_bytes: Option<u64>,
    /// The size the source actually holds.
    pub observed_bytes: Option<u64>,
    /// The first byte the source does not hold, where the session is
    /// bounded short of its declaration.
    pub first_unavailable_byte: Option<u64>,
}

impl Assurance {
    /// The assurance of an open with nothing against it: every fact its
    /// interpretation needs is present, and the whole medium reads.
    pub(crate) fn verified(observed_bytes: u64, access: AccessMode, claim: Claim) -> Self {
        Self {
            outcome: AssuranceOutcome::Verified,
            condition: None,
            evidence: Vec::new(),
            readable: vec![ByteRange::new(0, observed_bytes)],
            access,
            claim,
            declared_bytes: None,
            observed_bytes: Some(observed_bytes),
            first_unavailable_byte: None,
        }
    }

    /// Whether this session's authority was narrowed by evidence.
    pub fn is_degraded(&self) -> bool {
        self.outcome == AssuranceOutcome::Degraded
    }
}

/// The exact bound a degraded session reads under, carried where the reads
/// happen so no path can serve a byte the source does not hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ReadBound {
    /// The first byte that is not there — the end of the readable extent.
    pub(crate) end: u64,
    /// What the interpretation declared, which is what makes the shortfall
    /// nameable rather than merely a short file.
    pub(crate) declared: u64,
    pub(crate) condition: AssuranceCondition,
}

impl ReadBound {
    /// Refuses a read that would cross the bound, before it is attempted.
    pub(crate) fn check(&self, offset: u64, length: u64) -> Result<()> {
        let end = offset.saturating_add(length);
        if end <= self.end {
            return Ok(());
        }
        Err(self.unavailable(format!(
            "bytes {offset}..{end} are outside this medium's readable extent"
        )))
    }

    /// Refuses an operation that needs `needed` bytes of a medium whose
    /// readable extent stops short of it.
    pub(crate) fn withheld(&self, what: &str, needed: u64) -> Error {
        self.unavailable(format!(
            "{what} reaches byte {needed}, past this medium's readable extent"
        ))
    }

    fn unavailable(&self, what: String) -> Error {
        Error::unavailable(format!(
            "{what}: the source holds {} bytes where {} were declared, so \
             nothing from byte {} on is available to read",
            self.end, self.declared, self.end
        ))
        .broke_rule(self.condition.as_str())
    }
}

/// What a bounded interpretation of the medium declared, and what the
/// source turned out to hold — the two facts the gate compares.
pub(crate) struct Shortfall {
    pub(crate) declared: u64,
    pub(crate) observed: u64,
    /// The end of the structures the interpretation needs before it can
    /// name anything at all.
    pub(crate) metadata_end: u64,
}

/// The gate itself (P28): a declaration the source cannot satisfy narrows
/// the session to a degraded, read-only reading of the bytes that are
/// there.
pub(crate) fn degraded(shortfall: Shortfall, declaration: &str, claim: Claim) -> Assurance {
    let Shortfall {
        declared,
        observed,
        metadata_end,
    } = shortfall;
    let mut evidence = vec![
        declaration.to_owned(),
        format!(
            "the source holds {observed} bytes, {} short",
            declared - observed
        ),
        format!(
            "bytes 0..{observed} read; byte {observed} is the first the source does \
             not hold"
        ),
        "write authority is withheld for this session's whole life; only a new \
         open over a whole source establishes it again"
            .to_owned(),
    ];
    if metadata_end > observed {
        evidence.push(format!(
            "the interpretation's own leading structures end at byte \
             {metadata_end}, past what the source holds, so no entry is \
             addressable"
        ));
    }
    Assurance {
        outcome: AssuranceOutcome::Degraded,
        condition: Some(AssuranceCondition::SourceTruncated),
        evidence,
        readable: vec![ByteRange::new(0, observed)],
        access: AccessMode::ReadOnly,
        claim,
        declared_bytes: Some(declared),
        observed_bytes: Some(observed),
        first_unavailable_byte: Some(observed),
    }
}

/// The refusal a shortfall gets where the interpretation's own structure
/// contradicts itself: a bounded reading needs a bound, and there is none
/// to state (P28, P6).
pub(crate) fn conflicted(detail: &str, declared: u64, observed: u64) -> Error {
    Error::categorized_image(
        ErrorCategory::InvalidImage,
        "fat",
        format!(
            "{detail}; the source holds {observed} bytes against a declared \
             {declared}, and no safe bound can be stated for the difference, so \
             this medium is refused rather than read in part"
        ),
    )
    .broke_rule(AssuranceCondition::EvidenceConflict.as_str())
}

/// The refusal every mutation path of a degraded session returns.
pub(crate) fn read_only(assurance: &Assurance, path: &str) -> Error {
    let condition = assurance
        .condition
        .unwrap_or(AssuranceCondition::SourceTruncated);
    let shortfall = match (assurance.observed_bytes, assurance.declared_bytes) {
        (Some(observed), Some(declared)) => {
            format!("the source holds {observed} bytes where {declared} were declared")
        }
        _ => "the evidence beneath it falls short of its declaration".to_owned(),
    };
    Error::read_only(format!(
        "'{path}' opened degraded ({condition}) and is read-only for this \
         session's whole life: {shortfall}, so write authority was never \
         established"
    ))
    .broke_rule(condition.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outcome_and_condition_spellings_are_stable() {
        assert_eq!(AssuranceOutcome::Verified.as_str(), "verified");
        assert_eq!(AssuranceOutcome::Degraded.as_str(), "degraded");
        assert_eq!(AssuranceOutcome::Refused.as_str(), "refused");
        assert_eq!(
            AssuranceCondition::SourceTruncated.as_str(),
            "source-truncated"
        );
        assert_eq!(
            AssuranceCondition::EvidenceConflict.as_str(),
            "evidence-conflict"
        );
    }

    #[test]
    fn a_condition_reads_back_from_the_identity_a_refusal_carried() {
        for condition in AssuranceCondition::ALL {
            assert_eq!(
                AssuranceCondition::from_identity(condition.as_str()),
                Some(condition)
            );
        }
        assert_eq!(AssuranceCondition::from_identity("base-too-long"), None);
    }

    #[test]
    fn a_verified_open_reads_the_whole_medium_and_names_no_condition() {
        let assurance = Assurance::verified(4096, AccessMode::ReadWrite, Claim::LibraryOpened);
        assert_eq!(assurance.outcome, AssuranceOutcome::Verified);
        assert_eq!(assurance.condition, None);
        assert_eq!(assurance.readable, vec![ByteRange::new(0, 4096)]);
        assert_eq!(assurance.access, AccessMode::ReadWrite);
        assert!(!assurance.is_degraded());
    }

    #[test]
    fn the_gate_states_the_declaration_the_shortfall_and_the_withheld_authority() {
        let assurance = degraded(
            Shortfall {
                declared: 1_474_560,
                observed: 1_000_000,
                metadata_end: 16_896,
            },
            "the boot record declares 2880 sectors of 512 bytes",
            Claim::CallerOpened,
        );
        assert_eq!(assurance.outcome, AssuranceOutcome::Degraded);
        assert_eq!(
            assurance.condition,
            Some(AssuranceCondition::SourceTruncated)
        );
        assert_eq!(assurance.access, AccessMode::ReadOnly);
        assert_eq!(assurance.declared_bytes, Some(1_474_560));
        assert_eq!(assurance.observed_bytes, Some(1_000_000));
        assert_eq!(assurance.first_unavailable_byte, Some(1_000_000));
        assert_eq!(assurance.readable, vec![ByteRange::new(0, 1_000_000)]);
        assert!(
            assurance.evidence[0].contains("2880 sectors"),
            "the declaration leads the evidence: {:?}",
            assurance.evidence
        );
        assert!(
            assurance
                .evidence
                .iter()
                .all(|line| !line.contains("addressable")),
            "leading structures inside the extent raise nothing: {:?}",
            assurance.evidence
        );
    }

    #[test]
    fn leading_structures_past_the_extent_are_stated_as_evidence() {
        let assurance = degraded(
            Shortfall {
                declared: 1_474_560,
                observed: 4_096,
                metadata_end: 16_896,
            },
            "the boot record declares 2880 sectors of 512 bytes",
            Claim::LibraryOpened,
        );
        assert!(
            assurance
                .evidence
                .iter()
                .any(|line| line.contains("no entry is addressable")),
            "{:?}",
            assurance.evidence
        );
    }

    #[test]
    fn a_bound_refuses_a_crossing_read_and_names_the_condition() {
        let bound = ReadBound {
            end: 1_000_000,
            declared: 1_474_560,
            condition: AssuranceCondition::SourceTruncated,
        };
        assert!(bound.check(999_488, 512).is_ok(), "a read inside answers");
        let error = bound
            .check(999_999, 512)
            .expect_err("a crossing read is refused");
        assert_eq!(error.category(), ErrorCategory::Unavailable);
        assert_eq!(error.rule(), Some("source-truncated"));
        let message = error.to_string();
        assert!(
            message.contains("999999..1000511"),
            "names the range: {message}"
        );
        assert!(
            message.contains("1474560"),
            "names the declaration: {message}"
        );
    }

    #[test]
    fn a_conflict_refuses_rather_than_degrading() {
        let error = conflicted(
            "the boot record's two total-sector fields disagree",
            1_474_560,
            1_000_000,
        );
        assert_eq!(error.category(), ErrorCategory::InvalidImage);
        assert_eq!(error.rule(), Some("evidence-conflict"));
    }
}
