// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! Partition-layout adapters and their built-in catalog (P16).

use crate::device::Device;
use crate::error::Result;
use crate::mbr::{self, Discovery};

trait PartitionLayoutAdapter: Sync {
    fn discover(&self, device: &mut dyn Device) -> Result<Option<Discovery>>;
}

struct MbrAdapter;

impl PartitionLayoutAdapter for MbrAdapter {
    fn discover(&self, device: &mut dyn Device) -> Result<Option<Discovery>> {
        mbr::discover(device).map(Some)
    }
}

struct PartitionCatalog<'a> {
    adapters: &'a [&'a dyn PartitionLayoutAdapter],
}

impl PartitionCatalog<'_> {
    fn discover(&self, device: &mut dyn Device) -> Result<Discovery> {
        for adapter in self.adapters {
            if let Some(discovery) = adapter.discover(device)? {
                return Ok(discovery);
            }
        }
        Ok(Discovery::BareVolume)
    }
}

static MBR: MbrAdapter = MbrAdapter;
static BUILT_INS: [&dyn PartitionLayoutAdapter; 1] = [&MBR];

pub(crate) fn discover(device: &mut dyn Device) -> Result<Discovery> {
    PartitionCatalog {
        adapters: &BUILT_INS,
    }
    .discover(device)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::{Error, Result};

    struct TestAdapter;
    impl PartitionLayoutAdapter for TestAdapter {
        fn discover(&self, _device: &mut dyn Device) -> Result<Option<Discovery>> {
            Ok(Some(Discovery::Blank))
        }
    }

    struct Empty;
    impl Device for Empty {
        fn len(&self) -> u64 {
            0
        }
        fn read_at(&mut self, _offset: u64, _buf: &mut [u8]) -> Result<()> {
            Ok(())
        }
        fn write_at(&mut self, _offset: u64, _data: &[u8]) -> Result<()> {
            Err(Error::read_only("test device"))
        }
        fn flush(&mut self) -> Result<()> {
            Ok(())
        }
    }

    #[test]
    fn a_test_layout_is_reached_by_enrollment_alone() {
        let adapter = TestAdapter;
        let adapters: [&dyn PartitionLayoutAdapter; 1] = [&adapter];
        assert!(matches!(
            PartitionCatalog {
                adapters: &adapters
            }
            .discover(&mut Empty)
            .expect("discovers"),
            Discovery::Blank
        ));
    }
}
