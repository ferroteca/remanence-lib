// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! Filesystem adapters and the built-in filesystem catalog (P18), for
//! the namespaces a medium bears directly rather than through a composed
//! volume.
//!
//! Each adapter owns the bounded observations that distinguish its
//! filesystem. The catalog compares probe strength and never interprets a
//! named heuristic or branches on a filesystem identifier.
//!
//! **The adapter that recognized a namespace is the one that opens it.**
//! That is what keeps the resolver in [`crate::filesystem`] free of a
//! `match` on a filesystem id — a string-named rule in orchestration is
//! exactly what P12 and P18 keep out of it. An adapter that recognizes a
//! namespace this release does not read refuses at [`open`], by name
//! (P3).
//!
//! [`open`]: FilesystemAdapter::open

use crate::device::Device;
use crate::error::{Error, ErrorCategory, Result};
use crate::filesystem::{Catalog, HdosCatalog, SpaceRule};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FilesystemIdentification {
    pub(crate) filesystem_id: Option<&'static str>,
    pub(crate) filesystem_name: Option<&'static str>,
    pub(crate) confidence: u8,
    pub(crate) evidence: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ProbeResult {
    NoMatch,
    Match {
        confidence: u8,
        evidence: Vec<String>,
    },
}

pub(crate) trait FilesystemAdapter: Sync {
    fn id(&self) -> &'static str;
    fn name(&self) -> &'static str;
    fn probe(&self, volume: &mut dyn Device) -> Result<ProbeResult>;

    /// The namespace this adapter reads out of what it recognized, or
    /// the refusal naming a namespace this release recognizes and does
    /// not read (P3). Recognition and reading are separate claims, and
    /// the default is the honest one: an adapter that only probes says
    /// so here rather than being absent from the catalog.
    fn open(&self, _volume: &mut dyn Device) -> Result<Box<dyn Catalog>> {
        Err(Error::categorized_image(
            ErrorCategory::Unsupported,
            self.id(),
            format!(
                "this release recognizes {} and does not read its \
                     namespace",
                self.name()
            ),
        )
        .broke_rule(SpaceRule::RecognizedNotRead.as_str()))
    }
}

struct FilesystemCatalog<'a> {
    adapters: &'a [&'a dyn FilesystemAdapter],
}

impl<'a> FilesystemCatalog<'a> {
    const fn new(adapters: &'a [&'a dyn FilesystemAdapter]) -> Self {
        Self { adapters }
    }

    /// Every adapter that probed at the strongest confidence any of them
    /// reached, with its evidence. One entry is a recognition; several is
    /// a tie the catalog does not break.
    fn strongest(
        &self,
        volume: &mut dyn Device,
    ) -> Result<(u8, Vec<(&'a dyn FilesystemAdapter, Vec<String>)>)> {
        let mut strongest = 0;
        let mut candidates = Vec::new();
        for adapter in self.adapters {
            let ProbeResult::Match {
                confidence,
                evidence,
            } = adapter.probe(volume)?
            else {
                continue;
            };
            if confidence < strongest {
                continue;
            }
            if confidence > strongest {
                strongest = confidence;
                candidates.clear();
            }
            candidates.push((*adapter, evidence));
        }
        Ok((strongest, candidates))
    }

    fn detect(&self, volume: &mut dyn Device) -> Result<FilesystemIdentification> {
        let (strongest, mut candidates) = self.strongest(volume)?;

        Ok(match candidates.len() {
            0 => FilesystemIdentification {
                filesystem_id: None,
                filesystem_name: None,
                confidence: 0,
                evidence: vec!["no filesystem signatures found".to_owned()],
            },
            1 => {
                let (adapter, evidence) = candidates.pop().expect("one candidate");
                FilesystemIdentification {
                    filesystem_id: Some(adapter.id()),
                    filesystem_name: Some(adapter.name()),
                    confidence: strongest,
                    evidence,
                }
            }
            _ => {
                let mut evidence = vec![format!(
                    "filesystem recognition is ambiguous at confidence {strongest}"
                )];
                for (adapter, observations) in candidates {
                    evidence.push(format!("competing filesystem '{}':", adapter.id()));
                    evidence.extend(observations.into_iter().map(|item| format!("  {item}")));
                }
                FilesystemIdentification {
                    filesystem_id: None,
                    filesystem_name: None,
                    confidence: 0,
                    evidence,
                }
            }
        })
    }
}

struct HdosAdapter;

impl FilesystemAdapter for HdosAdapter {
    fn id(&self) -> &'static str {
        "hdos"
    }
    fn name(&self) -> &'static str {
        "Heath Disk Operating System"
    }

    fn probe(&self, volume: &mut dyn Device) -> Result<ProbeResult> {
        const CHUNK: usize = 4096;
        const MARKER: &[u8] = b"HDOS";
        let mut offset = 0u64;
        let mut carry = Vec::new();
        while offset < volume.len() {
            let take = (volume.len() - offset).min(CHUNK as u64) as usize;
            let mut chunk = vec![0u8; take];
            volume.read_at(offset, &mut chunk)?;
            carry.extend_from_slice(&chunk);
            if carry
                .windows(MARKER.len())
                .any(|window| window.eq_ignore_ascii_case(MARKER))
            {
                return Ok(ProbeResult::Match {
                    confidence: 80,
                    evidence: vec!["found ASCII marker 'HDOS'".to_owned()],
                });
            }
            if carry.len() >= MARKER.len() {
                carry.drain(..carry.len() - (MARKER.len() - 1));
            }
            offset += take as u64;
        }
        Ok(ProbeResult::NoMatch)
    }

    fn open(&self, volume: &mut dyn Device) -> Result<Box<dyn Catalog>> {
        Ok(Box::new(HdosCatalog::open(volume)?))
    }
}

fn is_cpm_name_byte(byte: u8) -> bool {
    let masked = byte & 0x7f;
    matches!(
        masked,
        b'A'..=b'Z' | b'0'..=b'9' | b' ' | b'!' | b'#' | b'$' | b'%'
            | b'&' | b'\'' | b'-' | b'@' | b'^' | b'_' | b'`' | b'{' | b'}' | b'~'
    )
}

fn looks_like_cpm_directory_entry(entry: &[u8]) -> bool {
    if entry.len() != 32 || entry[0] == 0xe5 || entry[0] > 15 {
        return false;
    }
    let name = &entry[1..12];
    name[..8].iter().any(|&byte| byte != b' ') && name.iter().copied().all(is_cpm_name_byte)
}

fn count_plausible_cpm_directory_entries(volume: &mut dyn Device) -> Result<usize> {
    const DIRECTORY_SIZE: usize = 2048;
    const OFFSETS: [u64; 5] = [0, 2560, 5120, 7680, 10240];
    let mut best = 0;
    for offset in OFFSETS {
        if offset + DIRECTORY_SIZE as u64 > volume.len() {
            continue;
        }
        let mut directory = [0u8; DIRECTORY_SIZE];
        volume.read_at(offset, &mut directory)?;
        best = best.max(
            directory
                .chunks_exact(32)
                .filter(|entry| looks_like_cpm_directory_entry(entry))
                .count(),
        );
    }
    Ok(best)
}

struct CpmAdapter;

impl FilesystemAdapter for CpmAdapter {
    fn id(&self) -> &'static str {
        "cpm"
    }
    fn name(&self) -> &'static str {
        "CP/M"
    }

    fn probe(&self, volume: &mut dyn Device) -> Result<ProbeResult> {
        let directory_entries = count_plausible_cpm_directory_entries(volume)?;
        if directory_entries < 2 {
            return Ok(ProbeResult::NoMatch);
        }
        Ok(ProbeResult::Match {
            confidence: (60 + directory_entries.min(8) * 5) as u8,
            evidence: vec![format!(
                "found {directory_entries} plausible CP/M directory entries"
            )],
        })
    }
}

static HDOS: HdosAdapter = HdosAdapter;
static CPM: CpmAdapter = CpmAdapter;
static BUILT_INS: [&dyn FilesystemAdapter; 2] = [&HDOS, &CPM];

pub(crate) fn detect(volume: &mut dyn Device) -> Result<FilesystemIdentification> {
    FilesystemCatalog::new(&BUILT_INS).detect(volume)
}

/// The enrolled adapter a declaration names, or the refusal naming what
/// this release reads (P3).
///
/// **A declared reading is resolved here and verified by the adapter it
/// names.** Nothing in this module picks a reading for a caller any
/// more: the catalog's probes exist to identify an artifact's layers,
/// and reading a namespace runs under a declaration.
pub(crate) fn by_id(id: &str) -> Result<&'static dyn FilesystemAdapter> {
    BUILT_INS
        .iter()
        .copied()
        .find(|adapter| adapter.id() == id)
        .ok_or_else(|| {
            let claimed: Vec<&str> = BUILT_INS.iter().map(|adapter| adapter.id()).collect();
            Error::categorized_io(
                ErrorCategory::Unsupported,
                format!(
                    "'{id}' names no namespace adapter enrolled here; the \
                     enrolled ones are {}",
                    claimed.join(", ")
                ),
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::{Error, Result};

    struct Bytes(Vec<u8>);

    impl Device for Bytes {
        fn len(&self) -> u64 {
            self.0.len() as u64
        }
        fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<()> {
            let start = offset as usize;
            let end = start + buf.len();
            let source = self
                .0
                .get(start..end)
                .ok_or_else(|| Error::io("past end"))?;
            buf.copy_from_slice(source);
            Ok(())
        }
        fn write_at(&mut self, _offset: u64, _data: &[u8]) -> Result<()> {
            Err(Error::read_only("test device"))
        }
        fn flush(&mut self) -> Result<()> {
            Ok(())
        }
    }

    fn write_cpm_entry(bytes: &mut [u8], offset: usize, name: &[u8; 11]) {
        bytes[offset..offset + 32].fill(0);
        bytes[offset + 1..offset + 12].copy_from_slice(name);
    }

    #[test]
    fn identifies_hdos_through_its_streamed_adapter() {
        let mut bytes = vec![0u8; 102_400];
        bytes[4095..4099].copy_from_slice(b"HDOS");
        let identification = detect(&mut Bytes(bytes)).expect("probes");
        assert_eq!(identification.filesystem_id, Some("hdos"));
    }

    #[test]
    fn identifies_cpm_through_its_adapter() {
        let mut bytes = vec![0xe5u8; 102_400];
        write_cpm_entry(&mut bytes, 5120, b"README  TXT");
        write_cpm_entry(&mut bytes, 5152, b"STAT    COM");
        let identification = detect(&mut Bytes(bytes)).expect("probes");
        assert_eq!(identification.filesystem_id, Some("cpm"));
    }

    struct TestAdapter;
    impl FilesystemAdapter for TestAdapter {
        fn id(&self) -> &'static str {
            "test"
        }
        fn name(&self) -> &'static str {
            "Test filesystem"
        }
        fn probe(&self, _volume: &mut dyn Device) -> Result<ProbeResult> {
            Ok(ProbeResult::Match {
                confidence: 77,
                evidence: vec!["test evidence".to_owned()],
            })
        }
    }

    #[test]
    fn a_test_adapter_is_recognized_by_catalog_enrollment_alone() {
        let adapter = TestAdapter;
        let adapters: [&dyn FilesystemAdapter; 1] = [&adapter];
        let found = FilesystemCatalog::new(&adapters)
            .detect(&mut Bytes(Vec::new()))
            .expect("probes");
        assert_eq!(found.filesystem_id, Some("test"));
    }
}
