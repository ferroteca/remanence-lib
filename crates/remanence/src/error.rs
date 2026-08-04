// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

use std::fmt;

/// Stable, machine-readable classification of a library refusal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCategory {
    Locked,
    InvalidImage,
    Unsupported,
    ReadOnly,
    NotFound,
    NotDirectory,
    IsDirectory,
    NoSpace,
    Io,
}

impl ErrorCategory {
    /// The stable cross-language spelling of this category.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Locked => "locked",
            Self::InvalidImage => "invalid-image",
            Self::Unsupported => "unsupported",
            Self::ReadOnly => "read-only",
            Self::NotFound => "not-found",
            Self::NotDirectory => "not-directory",
            Self::IsDirectory => "is-directory",
            Self::NoSpace => "no-space",
            Self::Io => "io",
        }
    }
}

impl fmt::Display for ErrorCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Which rule of an enumerated set an input broke, in the stable spelling
/// owned by the seam that defines those rules (P10).
///
/// This is deliberately a bare identity rather than a second global enum.
/// The category set is small and cross-cutting because it answers *how
/// should the caller behave* for the whole library at once; a rule set
/// answers *which rule did this input break* and belongs to the format,
/// namespace, or grammar that defines it — `DosNameRule` is the first one.
/// A refusal belonging to no such set carries `None`, which is ordinary
/// rather than an omission.
pub type RuleIdentity = &'static str;

/// Library error type. Each variant records the data required to produce its
/// display message, its stable machine-readable category, and the rule
/// identity where the refusal came from an enumerated rule set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// A container archive (e.g. ZIP, 7z) could not be read.
    Archive {
        category: ErrorCategory,
        rule: Option<RuleIdentity>,
        archive: String,
        reason: String,
    },
    /// An underlying I/O operation failed.
    Io {
        category: ErrorCategory,
        rule: Option<RuleIdentity>,
        reason: String,
    },
    /// A disk image did not match its container format.
    InvalidImage {
        category: ErrorCategory,
        rule: Option<RuleIdentity>,
        container: String,
        reason: String,
    },
}

impl Error {
    pub fn archive(archive: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::categorized_archive(ErrorCategory::InvalidImage, archive, reason)
    }

    pub(crate) fn categorized_archive(
        category: ErrorCategory,
        archive: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        Self::Archive {
            category,
            rule: None,
            archive: archive.into(),
            reason: reason.into(),
        }
    }

    pub fn io(reason: impl Into<String>) -> Self {
        Self::categorized_io(ErrorCategory::Io, reason)
    }

    pub(crate) fn locked(reason: impl Into<String>) -> Self {
        Self::categorized_io(ErrorCategory::Locked, reason)
    }

    pub(crate) fn read_only(reason: impl Into<String>) -> Self {
        Self::categorized_io(ErrorCategory::ReadOnly, reason)
    }

    pub(crate) fn not_found(reason: impl Into<String>) -> Self {
        Self::categorized_io(ErrorCategory::NotFound, reason)
    }

    /// A refusal that names something outside the library's claim, where
    /// no format has been recognized yet to attribute it to.
    pub(crate) fn unsupported(reason: impl Into<String>) -> Self {
        Self::categorized_io(ErrorCategory::Unsupported, reason)
    }

    fn categorized_io(category: ErrorCategory, reason: impl Into<String>) -> Self {
        Self::Io {
            category,
            rule: None,
            reason: reason.into(),
        }
    }

    pub fn invalid_image(container: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::categorized_image(ErrorCategory::InvalidImage, container, reason)
    }

    pub(crate) fn categorized_image(
        category: ErrorCategory,
        container: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        Self::InvalidImage {
            category,
            rule: None,
            container: container.into(),
            reason: reason.into(),
        }
    }

    /// Names the rule of an enumerated set this refusal broke. The
    /// identity sits beside the category and never substitutes for it: the
    /// category still says how the caller should behave, and this says
    /// which rule. Rule sets are owned by their seam, so the value passed
    /// here comes from that seam's own enumeration —
    /// [`DosNameRule::as_str`](crate::DosNameRule::as_str) for the DOS 8.3
    /// namespace's.
    pub fn broke_rule(self, rule: RuleIdentity) -> Self {
        match self {
            Self::Archive {
                category,
                archive,
                reason,
                ..
            } => Self::Archive {
                category,
                rule: Some(rule),
                archive,
                reason,
            },
            Self::Io {
                category, reason, ..
            } => Self::Io {
                category,
                rule: Some(rule),
                reason,
            },
            Self::InvalidImage {
                category,
                container,
                reason,
                ..
            } => Self::InvalidImage {
                category,
                rule: Some(rule),
                container,
                reason,
            },
        }
    }

    /// The stable category callers should use to branch on this error.
    pub const fn category(&self) -> ErrorCategory {
        match self {
            Self::Archive { category, .. }
            | Self::Io { category, .. }
            | Self::InvalidImage { category, .. } => *category,
        }
    }

    /// Which rule this refusal broke, where it came from an enumerated rule
    /// set, and `None` where no such set applies. The identity is stable
    /// and machine-readable; the seam owning the set is what gives it
    /// meaning, and [`crate::DosNameRule`] reads the DOS 8.3 namespace's.
    pub const fn rule(&self) -> Option<RuleIdentity> {
        match self {
            Self::Archive { rule, .. }
            | Self::Io { rule, .. }
            | Self::InvalidImage { rule, .. } => *rule,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Archive {
                archive, reason, ..
            } => {
                write!(f, "invalid {archive} archive: {reason}")
            }
            Self::Io { reason, .. } => write!(f, "{reason}"),
            Self::InvalidImage {
                container, reason, ..
            } => {
                write!(f, "invalid {container} disk image: {reason}")
            }
        }
    }
}

impl std::error::Error for Error {}

/// Library result alias.
pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn category_spellings_are_stable() {
        assert_eq!(ErrorCategory::Locked.as_str(), "locked");
        assert_eq!(ErrorCategory::InvalidImage.as_str(), "invalid-image");
        assert_eq!(ErrorCategory::Unsupported.as_str(), "unsupported");
        assert_eq!(ErrorCategory::ReadOnly.as_str(), "read-only");
        assert_eq!(ErrorCategory::NotFound.as_str(), "not-found");
        assert_eq!(ErrorCategory::NotDirectory.as_str(), "not-directory");
        assert_eq!(ErrorCategory::IsDirectory.as_str(), "is-directory");
        assert_eq!(ErrorCategory::NoSpace.as_str(), "no-space");
        assert_eq!(ErrorCategory::Io.as_str(), "io");
    }

    #[test]
    fn category_does_not_change_the_diagnostic() {
        let error =
            Error::categorized_image(ErrorCategory::NotFound, "fat", "'FILE.TXT' not found");
        assert_eq!(error.category(), ErrorCategory::NotFound);
        assert_eq!(
            error.to_string(),
            "invalid fat disk image: 'FILE.TXT' not found"
        );
    }

    #[test]
    fn a_refusal_outside_any_rule_set_carries_no_rule_identity() {
        assert_eq!(Error::io("failed").rule(), None);
        assert_eq!(Error::archive("zip", "malformed").rule(), None);
        assert_eq!(Error::invalid_image("fat", "malformed").rule(), None);
    }

    #[test]
    fn a_rule_identity_sits_beside_the_category_without_replacing_it() {
        let error = Error::io("'CON' names a reserved device").broke_rule("reserved-device-name");
        assert_eq!(error.category(), ErrorCategory::Io);
        assert_eq!(error.rule(), Some("reserved-device-name"));
        assert_eq!(error.to_string(), "'CON' names a reserved device");
    }

    #[test]
    fn every_variant_can_carry_a_rule_identity() {
        assert_eq!(
            Error::archive("zip", "malformed").broke_rule("some-rule").rule(),
            Some("some-rule")
        );
        assert_eq!(
            Error::invalid_image("fat", "malformed")
                .broke_rule("some-rule")
                .rule(),
            Some("some-rule")
        );
    }

    #[test]
    fn existing_error_families_have_deliberate_default_categories() {
        assert_eq!(
            Error::archive("zip", "malformed").category(),
            ErrorCategory::InvalidImage
        );
        assert_eq!(
            Error::invalid_image("fat", "malformed").category(),
            ErrorCategory::InvalidImage
        );
        assert_eq!(Error::io("failed").category(), ErrorCategory::Io);
    }
}
