// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

use std::path::Path;

use crate::registry::FormatRegistry;

/// The best container match for a byte buffer, with supporting evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ContainerIdentification {
    pub container_id: Option<String>,
    pub container_name: Option<String>,
    pub confidence: u8,
    pub evidence: Vec<String>,
}

/// Scores every registered container format against the image bytes and the
/// (optional) file name, returning the highest-confidence match.
pub(crate) fn detect(
    bytes: &[u8],
    file_name: Option<&Path>,
    registry: &FormatRegistry,
) -> ContainerIdentification {
    let extension = file_name
        .and_then(Path::extension)
        .map(|extension| extension.to_string_lossy().to_ascii_lowercase())
        .filter(|extension| !extension.is_empty());

    let mut best = ContainerIdentification {
        container_id: None,
        container_name: None,
        confidence: 0,
        evidence: vec!["no container signatures found".to_owned()],
    };

    for container in registry.containers().values() {
        let mut confidence: u8 = 0;
        let mut evidence = Vec::new();

        if let Some(expected_size) = container.expected_size() {
            if bytes.len() == expected_size {
                confidence = confidence.saturating_add(80);
                evidence.push(format!("matched expected size of {expected_size} bytes"));
            }
        }

        if let Some(magic) = &container.magic {
            if bytes.starts_with(magic) {
                confidence = confidence.saturating_add(80);
                evidence.push(format!(
                    "matched {}-byte magic signature",
                    magic.len()
                ));
            }
        }

        if let Some(extension) = &extension {
            let matched = container
                .extensions
                .iter()
                .any(|candidate| candidate.eq_ignore_ascii_case(extension));
            if matched {
                confidence = confidence.saturating_add(20);
                evidence.push(format!("matched file extension '.{extension}'"));
            }
        }

        if confidence > best.confidence {
            best = ContainerIdentification {
                container_id: Some(container.id.clone()),
                container_name: Some(container.name.clone()),
                confidence: confidence.min(100),
                evidence,
            };
        }
    }

    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::default_format_registry;

    #[test]
    fn identifies_h8d_by_size_and_extension() {
        let registry = default_format_registry().expect("default registry");

        let bytes = vec![0u8; 102_400];
        let identification = detect(&bytes, Some(Path::new("disk.h8d")), &registry);

        assert_eq!(identification.container_id.as_deref(), Some("h8d"));
        assert_eq!(
            identification.container_name.as_deref(),
            Some("Heathkit H8 H17 disk image")
        );
        assert_eq!(identification.confidence, 100);
    }

    #[test]
    fn returns_no_container_when_metadata_does_not_match() {
        let registry = default_format_registry().expect("default registry");

        let bytes = vec![0u8; 10];
        let identification = detect(&bytes, Some(Path::new("disk.bin")), &registry);

        assert_eq!(identification.container_id, None);
        assert_eq!(identification.container_name, None);
        assert_eq!(identification.confidence, 0);
    }
}
