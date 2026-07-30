// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

use std::fmt;

/// Library error type. Each variant records the data required to produce its
/// display message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// A container archive (e.g. ZIP) could not be read.
    Archive { archive: String, reason: String },
    /// An underlying I/O operation failed.
    Io { reason: String },
    /// A disk image did not match its container format.
    InvalidImage { container: String, reason: String },
    /// The format registry definition text could not be parsed.
    Registry { line: usize, reason: String },
    /// A container id was not present in the registry.
    UnknownContainer { id: String },
}

impl Error {
    pub fn archive(archive: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::Archive { archive: archive.into(), reason: reason.into() }
    }

    pub fn io(reason: impl Into<String>) -> Self {
        Self::Io { reason: reason.into() }
    }

    pub fn invalid_image(container: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::InvalidImage { container: container.into(), reason: reason.into() }
    }

    pub fn registry(line: usize, reason: impl Into<String>) -> Self {
        Self::Registry { line, reason: reason.into() }
    }

    pub fn unknown_container(id: impl Into<String>) -> Self {
        Self::UnknownContainer { id: id.into() }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Archive { archive, reason } => {
                write!(f, "invalid {archive} archive: {reason}")
            }
            Self::Io { reason } => write!(f, "{reason}"),
            Self::InvalidImage { container, reason } => {
                write!(f, "invalid {container} disk image: {reason}")
            }
            Self::Registry { line, reason } => {
                write!(f, "format registry parse error on line {line}: {reason}")
            }
            Self::UnknownContainer { id } => {
                write!(f, "unknown container format '{id}'")
            }
        }
    }
}

impl std::error::Error for Error {}

/// Library result alias.
pub type Result<T> = std::result::Result<T, Error>;
