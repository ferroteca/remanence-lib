// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! Resolves a user-supplied path — a raw image, or `archive.zip[/entry]` — to
//! the image bytes plus any archive layers that were unwrapped along the way.
//! The source file is opened under the P7 claim, and the claim travels with
//! the resolution so the session can hold it for its lifetime.

use std::fs::File;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

use crate::device::{AccessMode, open_locked};
use crate::error::{Error, Result};
use crate::zip::ZipArchive;

/// One archive wrapper that was unwrapped while resolving an image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ArchiveLayer {
    pub id: String,
    pub name: String,
    pub path: PathBuf,
    pub entry_name: String,
    pub archive_size: Option<u64>,
    pub compressed_size: Option<u64>,
    pub uncompressed_size: Option<u64>,
}

/// The P7 claim on the source file, held for the session's lifetime.
#[derive(Debug)]
pub(crate) struct SourceClaim {
    _file: File,
    pub mode: AccessMode,
}

/// The fully-resolved image bytes and provenance.
#[derive(Debug)]
pub(crate) struct ResolvedImage {
    pub source_path: PathBuf,
    pub image_path: PathBuf,
    pub bytes: Vec<u8>,
    pub archive_layers: Vec<ArchiveLayer>,
    pub claim: SourceClaim,
}

fn has_zip_extension(component: &Path) -> bool {
    component
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("zip"))
}

/// Splits a path at the first `.zip` component into
/// `(archive_path, optional entry_path)`.
fn split_zip_path(path: &Path) -> Option<(PathBuf, Option<PathBuf>)> {
    let mut archive_path = PathBuf::new();
    let mut entry_path = PathBuf::new();
    let mut found_archive = false;

    for component in path.components() {
        if found_archive {
            if matches!(component, Component::CurDir) {
                continue;
            }
            entry_path.push(component.as_os_str());
            continue;
        }

        archive_path.push(component.as_os_str());
        if has_zip_extension(Path::new(component.as_os_str())) {
            found_archive = true;
        }
    }

    if !found_archive {
        return None;
    }

    let entry = (!entry_path.as_os_str().is_empty()).then_some(entry_path);
    Some((archive_path, entry))
}

/// Joins the normal components of an entry path with `/`.
fn normalize_entry_name(path: &Path) -> String {
    let mut result = String::new();
    for component in path.components() {
        let Component::Normal(component) = component else {
            continue;
        };
        if !result.is_empty() {
            result.push('/');
        }
        result.push_str(&component.to_string_lossy());
    }
    result
}

/// Opens `path` under the P7 ladder and reads it whole through the
/// claimed handle.
fn read_claimed(path: &Path) -> Result<(Vec<u8>, SourceClaim)> {
    let (mut file, mode) = open_locked(path)?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|error| Error::io(format!("failed to read '{}': {error}", path.display())))?;
    Ok((bytes, SourceClaim { _file: file, mode }))
}

/// Returns the name of the archive's only file entry, or an error when the
/// archive is empty or ambiguous.
fn only_file_entry_name(archive: &ZipArchive, archive_path: &Path) -> Result<String> {
    let mut file_entry_name: Option<&str> = None;
    for entry in archive.entries() {
        if entry.is_dir {
            continue;
        }
        if file_entry_name.is_some() {
            return Err(Error::archive(
                "zip",
                format!(
                    "'{}' contains multiple files; specify an entry with archive.zip/path",
                    archive_path.display()
                ),
            ));
        }
        file_entry_name = Some(&entry.name);
    }

    file_entry_name
        .map(str::to_owned)
        .ok_or_else(|| Error::archive("zip", "archive contains no files"))
}

/// Resolves `path` to image bytes, unwrapping a `.zip` archive when present.
pub(crate) fn resolve_image(path: &Path) -> Result<ResolvedImage> {
    let Some((archive_path, entry_path)) = split_zip_path(path) else {
        let (bytes, claim) = read_claimed(path)?;
        return Ok(ResolvedImage {
            source_path: path.to_path_buf(),
            image_path: path.to_path_buf(),
            bytes,
            archive_layers: Vec::new(),
            claim,
        });
    };

    let (archive_bytes, claim) = read_claimed(&archive_path)?;
    let archive_size = Some(archive_bytes.len() as u64);
    let archive = ZipArchive::from_bytes(archive_bytes)?;

    let entry_name = match &entry_path {
        Some(entry_path) => normalize_entry_name(entry_path),
        None => only_file_entry_name(&archive, &archive_path)?,
    };

    let entry = archive
        .entries()
        .iter()
        .find(|entry| entry.name == entry_name)
        .ok_or_else(|| Error::archive("zip", format!("entry '{entry_name}' not found")))?;

    let compressed_size = entry.compressed_size;
    let uncompressed_size = entry.uncompressed_size;
    let bytes = archive.read_entry(entry)?;

    let layer = ArchiveLayer {
        id: "zip".to_owned(),
        name: "ZIP archive".to_owned(),
        path: archive_path.clone(),
        entry_name: entry_name.clone(),
        archive_size,
        compressed_size: Some(compressed_size),
        uncompressed_size: Some(uncompressed_size),
    };

    Ok(ResolvedImage {
        source_path: archive_path,
        image_path: PathBuf::from(entry_name),
        bytes,
        archive_layers: vec![layer],
        claim,
    })
}
