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

/// Library error type. Each variant records the data required to produce its
/// display message and its stable machine-readable category.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// A container archive (e.g. ZIP) could not be read.
    Archive {
        category: ErrorCategory,
        archive: String,
        reason: String,
    },
    /// An underlying I/O operation failed.
    Io {
        category: ErrorCategory,
        reason: String,
    },
    /// A disk image did not match its container format.
    InvalidImage {
        category: ErrorCategory,
        container: String,
        reason: String,
    },
    /// The format registry definition text could not be parsed.
    Registry {
        category: ErrorCategory,
        line: usize,
        reason: String,
    },
    /// A container id was not present in the registry.
    UnknownContainer { category: ErrorCategory, id: String },
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

    fn categorized_io(category: ErrorCategory, reason: impl Into<String>) -> Self {
        Self::Io {
            category,
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
            container: container.into(),
            reason: reason.into(),
        }
    }

    pub fn registry(line: usize, reason: impl Into<String>) -> Self {
        Self::Registry {
            category: ErrorCategory::InvalidImage,
            line,
            reason: reason.into(),
        }
    }

    pub fn unknown_container(id: impl Into<String>) -> Self {
        Self::UnknownContainer {
            category: ErrorCategory::Unsupported,
            id: id.into(),
        }
    }

    /// The stable category callers should use to branch on this error.
    pub const fn category(&self) -> ErrorCategory {
        match self {
            Self::Archive { category, .. }
            | Self::Io { category, .. }
            | Self::InvalidImage { category, .. }
            | Self::Registry { category, .. }
            | Self::UnknownContainer { category, .. } => *category,
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
            Self::Registry { line, reason, .. } => {
                write!(f, "format registry parse error on line {line}: {reason}")
            }
            Self::UnknownContainer { id, .. } => {
                write!(f, "unknown container format '{id}'")
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
    fn existing_error_families_have_deliberate_default_categories() {
        assert_eq!(
            Error::archive("zip", "malformed").category(),
            ErrorCategory::InvalidImage
        );
        assert_eq!(
            Error::invalid_image("fat", "malformed").category(),
            ErrorCategory::InvalidImage
        );
        assert_eq!(
            Error::registry(1, "malformed").category(),
            ErrorCategory::InvalidImage
        );
        assert_eq!(
            Error::unknown_container("future").category(),
            ErrorCategory::Unsupported
        );
        assert_eq!(Error::io("failed").category(), ErrorCategory::Io);
    }
}
