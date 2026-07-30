// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

use crate::error::{Error, Result};
use crate::registry::ContainerFormat;

/// A raw disk image validated against its container format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiskImage {
    container_id: String,
    bytes: Vec<u8>,
}

impl DiskImage {
    pub fn from_bytes(container: &ContainerFormat, bytes: Vec<u8>) -> Result<Self> {
        if let Some(expected_size) = container.expected_size() {
            if bytes.len() != expected_size {
                return Err(Error::invalid_image(
                    &container.id,
                    format!("expected {expected_size} bytes, found {}", bytes.len()),
                ));
            }
        }

        Ok(Self { container_id: container.id.clone(), bytes })
    }

    pub fn container_id(&self) -> &str {
        &self.container_id
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::default_format_registry;

    #[test]
    fn validates_h8d_size_from_registry_metadata() {
        let registry = default_format_registry().expect("default registry");
        let h8d = registry.container("h8d").expect("h8d container");

        assert!(DiskImage::from_bytes(h8d, vec![0; 102_400]).is_ok());
        assert!(DiskImage::from_bytes(h8d, vec![0; 102_399]).is_err());
    }
}
