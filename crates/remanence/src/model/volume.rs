// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The volume-composition seam (P17).
//!
//! The delivered seam supplies only the direct one-region composition. It is still an
//! adapter boundary: filesystems consume the resulting logical volume and
//! do not learn whether a partition layout preceded it.

use crate::image::adapters::DeviceIdentity;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AddressedRegion {
    pub(crate) device: DeviceIdentity,
    pub(crate) offset: u64,
    pub(crate) length: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LogicalVolume {
    pub(crate) device: DeviceIdentity,
    pub(crate) offset: u64,
    pub(crate) length: u64,
}

trait VolumeCompositionAdapter {
    fn compose(&self, regions: &[AddressedRegion]) -> Option<LogicalVolume>;
}

struct DirectVolume;

impl VolumeCompositionAdapter for DirectVolume {
    fn compose(&self, regions: &[AddressedRegion]) -> Option<LogicalVolume> {
        let [region] = regions else { return None };
        Some(LogicalVolume {
            device: region.device,
            offset: region.offset,
            length: region.length,
        })
    }
}

pub(crate) fn direct(region: AddressedRegion) -> LogicalVolume {
    DirectVolume.compose(&[region]).expect("one direct region")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direct_composition_preserves_disk_local_addressing() {
        let region = AddressedRegion {
            device: DeviceIdentity::first(),
            offset: 4096,
            length: 8192,
        };
        let volume = direct(region);
        assert_eq!(volume.device, region.device);
        assert_eq!(volume.offset, 4096);
        assert_eq!(volume.length, 8192);
    }
}
