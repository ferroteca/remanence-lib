// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

use crate::image::DiskImage;
use crate::registry::FormatRegistry;

/// The best filesystem match for a disk image, with supporting evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FilesystemIdentification {
    pub filesystem_id: Option<String>,
    pub filesystem_name: Option<String>,
    pub confidence: u8,
    pub evidence: Vec<String>,
}

fn window_contains_ignore_ascii_case(haystack: &[u8], needle: &str) -> bool {
    let needle = needle.as_bytes();
    if needle.is_empty() || haystack.len() < needle.len() {
        return false;
    }
    haystack
        .windows(needle.len())
        .any(|window| window.eq_ignore_ascii_case(needle))
}

fn is_cpm_name_byte(byte: u8) -> bool {
    let masked = byte & 0x7f;
    matches!(
        masked,
        b'A'..=b'Z'
            | b'0'..=b'9'
            | b' '
            | b'!'
            | b'#'
            | b'$'
            | b'%'
            | b'&'
            | b'\''
            | b'-'
            | b'@'
            | b'^'
            | b'_'
            | b'`'
            | b'{'
            | b'}'
            | b'~'
    )
}

fn looks_like_cpm_directory_entry(entry: &[u8]) -> bool {
    if entry.len() != 32 || entry[0] == 0xe5 || entry[0] > 15 {
        return false;
    }

    let name = &entry[1..12];
    let has_name = name[..8].iter().any(|&byte| byte != b' ');
    if !has_name {
        return false;
    }
    name.iter().copied().all(is_cpm_name_byte)
}

fn count_plausible_cpm_directory_entries(bytes: &[u8]) -> usize {
    const ENTRY_SIZE: usize = 32;
    const DIRECTORY_SIZE: usize = 2048;
    const OFFSETS: [usize; 5] = [0, 2560, 5120, 7680, 10240];

    let mut best = 0;
    for offset in OFFSETS {
        if offset + DIRECTORY_SIZE > bytes.len() {
            continue;
        }
        let directory = &bytes[offset..offset + DIRECTORY_SIZE];
        let count = directory
            .chunks_exact(ENTRY_SIZE)
            .filter(|entry| looks_like_cpm_directory_entry(entry))
            .count();
        best = best.max(count);
    }
    best
}

fn score_filesystem(
    bytes: &[u8],
    filesystem_id: &str,
    filesystem_name: &str,
    heuristics: &[String],
    markers: &[String],
) -> FilesystemIdentification {
    let mut confidence: u8 = 0;
    let mut evidence = Vec::new();

    for heuristic in heuristics {
        match heuristic.as_str() {
            "ascii-marker" => {
                for marker in markers {
                    if window_contains_ignore_ascii_case(bytes, marker) {
                        confidence = confidence.saturating_add(80);
                        evidence.push(format!("found ASCII marker '{marker}'"));
                    }
                }
            }
            "cpm-directory" => {
                let directory_entries = count_plausible_cpm_directory_entries(bytes);
                if directory_entries >= 2 {
                    let bonus = (directory_entries.min(8) * 5) as u8;
                    confidence = confidence.saturating_add(60 + bonus);
                    evidence.push(format!(
                        "found {directory_entries} plausible CP/M directory entries"
                    ));
                }
            }
            _ => {}
        }
    }

    let matched = confidence > 0;
    FilesystemIdentification {
        filesystem_id: matched.then(|| filesystem_id.to_owned()),
        filesystem_name: matched.then(|| filesystem_name.to_owned()),
        confidence: confidence.min(100),
        evidence,
    }
}

/// Scores the container's candidate filesystems against the image bytes,
/// returning the highest-confidence match.
pub(crate) fn detect(
    image: &DiskImage,
    registry: &FormatRegistry,
) -> FilesystemIdentification {
    let Some(container) = registry.container(image.container_id()) else {
        return FilesystemIdentification {
            filesystem_id: None,
            filesystem_name: None,
            confidence: 0,
            evidence: vec![format!("unknown container '{}'", image.container_id())],
        };
    };

    let mut best = FilesystemIdentification {
        filesystem_id: None,
        filesystem_name: None,
        confidence: 0,
        evidence: vec!["no filesystem signatures found".to_owned()],
    };

    for candidate_id in &container.filesystem_candidates {
        let Some(filesystem) = registry.filesystem(candidate_id) else {
            continue;
        };

        let identification = score_filesystem(
            image.bytes(),
            &filesystem.id,
            &filesystem.name,
            &filesystem.heuristics,
            &filesystem.markers,
        );
        if identification.confidence > best.confidence {
            best = identification;
        }
    }

    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::default_format_registry;

    fn write_cpm_entry(bytes: &mut [u8], offset: usize, name: &[u8; 11]) {
        bytes[offset..offset + 32].fill(0);
        bytes[offset + 1..offset + 12].copy_from_slice(name);
    }

    #[test]
    fn identifies_hdos_from_registry_marker() {
        let registry = default_format_registry().expect("default registry");
        let h8d = registry.container("h8d").expect("h8d container");

        let mut bytes = vec![0u8; h8d.expected_size().expect("expected size")];
        bytes[128..132].copy_from_slice(b"HDOS");
        let image = DiskImage::from_bytes(h8d, bytes).expect("valid image");

        let identification = detect(&image, &registry);

        assert_eq!(identification.filesystem_id.as_deref(), Some("hdos"));
        assert_eq!(
            identification.filesystem_name.as_deref(),
            Some("Heath Disk Operating System")
        );
        assert!(identification.confidence >= 80);
    }

    #[test]
    fn identifies_cpm_from_directory_heuristic() {
        let registry = default_format_registry().expect("default registry");
        let h8d = registry.container("h8d").expect("h8d container");

        let mut bytes = vec![0xe5u8; h8d.expected_size().expect("expected size")];
        write_cpm_entry(&mut bytes, 5120, b"README  TXT");
        write_cpm_entry(&mut bytes, 5152, b"STAT    COM");
        let image = DiskImage::from_bytes(h8d, bytes).expect("valid image");

        let identification = detect(&image, &registry);

        assert_eq!(identification.filesystem_id.as_deref(), Some("cpm"));
        assert_eq!(identification.filesystem_name.as_deref(), Some("CP/M"));
        assert!(identification.confidence >= 70);
    }
}
