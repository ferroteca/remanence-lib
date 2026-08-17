// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The linear extent an FM or MFM recording composes (D62).
//!
//! **Why there is one at all.** A CBM DOS recording's blocks are
//! addressed by the recording, so its sector layer composes no addressed
//! extent and the only namespace declarable over it is `cbmdos`. An FM
//! or MFM recording is the other case: its records state a cylinder, a
//! head and a sector number, and those compose exactly the geometry
//! ordering FAT, HDOS and CP/M were all written against. So this layer
//! presents a [`Device`] — the same byte-shaped seam a hard-disk image
//! opens through — and the filesystem adapters read it without either
//! side learning about the other.
//!
//! **The geometry is the recording's, not the profile's.** A drive
//! profile declares a nominal geometry; what a particular disk holds is
//! what its records say. Every number here is read off the claims, and a
//! recording whose records do not compose a uniform image is refused by
//! name showing what it states rather than flattened into an ordering
//! that would put every file's contents somewhere other than where they
//! are.
//!
//! **A hole is a hole.** A sector the recording never stated, or one
//! whose CRC disagrees, has no bytes. Reads that touch it are refused
//! naming the address; reads that do not are answered. Nothing is
//! zeroed, because a zero here is indistinguishable from a zero the
//! recording actually holds.

use crate::Result;
use crate::error::{Error, ErrorCategory};
use crate::flux::ibm::sectors::IbmSectors;
use crate::io::device::Device;

/// The uniform geometry a recording's own records state.
///
/// Every field is derived from the claims. Nothing here is the drive
/// profile's nominal figure: a profile says what the mechanism records,
/// and this says what this disk holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IbmGeometry {
    /// How many cylinders the records span, counted from zero.
    pub cylinders: u32,
    /// How many heads, counted from zero.
    pub heads: u32,
    /// How many records one track holds.
    pub sectors_per_track: u32,
    /// The lowest sector number the records state. IBM recordings
    /// conventionally number from one, but that is a convention and this
    /// reads what is there.
    pub first_sector: u8,
    /// How many bytes one record's data field holds.
    pub sector_bytes: u32,
}

impl IbmGeometry {
    /// The whole extent's length, which is the geometry's rather than
    /// the sum of what reads: a hole still occupies its place.
    pub fn length_bytes(&self) -> u64 {
        u64::from(self.cylinders)
            * u64::from(self.heads)
            * u64::from(self.sectors_per_track)
            * u64::from(self.sector_bytes)
    }

    /// Where one record's data field begins in the extent.
    ///
    /// This is the ordering every filesystem that reads a floppy was
    /// written against: sectors within a track, then heads within a
    /// cylinder, then cylinders.
    fn offset_of(&self, cylinder: u8, head: u8, sector: u8) -> Option<u64> {
        let cylinder = u32::from(cylinder);
        let head = u32::from(head);
        if cylinder >= self.cylinders || head >= self.heads {
            return None;
        }
        let index = sector.checked_sub(self.first_sector)?;
        let index = u32::from(index);
        if index >= self.sectors_per_track {
            return None;
        }
        let block = (cylinder * self.heads + head) * self.sectors_per_track + index;
        Some(u64::from(block) * u64::from(self.sector_bytes))
    }

    /// Which record holds the byte at `offset`, and how far into it.
    fn record_at(&self, offset: u64) -> Option<(u8, u8, u8, usize)> {
        if offset >= self.length_bytes() {
            return None;
        }
        let block = offset / u64::from(self.sector_bytes);
        let within = (offset % u64::from(self.sector_bytes)) as usize;
        let per_cylinder = u64::from(self.heads) * u64::from(self.sectors_per_track);
        let cylinder = block / per_cylinder;
        let rest = block % per_cylinder;
        let head = rest / u64::from(self.sectors_per_track);
        let index = rest % u64::from(self.sectors_per_track);
        Some((
            u8::try_from(cylinder).ok()?,
            u8::try_from(head).ok()?,
            self.first_sector.checked_add(u8::try_from(index).ok()?)?,
            within,
        ))
    }
}

fn refuse(reason: impl Into<String>) -> Error {
    Error::categorized_image(ErrorCategory::Unsupported, "ibm", reason)
}

/// Derives the uniform geometry a recording's records state, or refuses
/// naming what makes them non-uniform.
///
/// The three things a linear image needs are checked separately so the
/// refusal says which one failed: one data-field size, one contiguous
/// run of sector numbers, and no cylinder or head missing from the span
/// the records describe.
pub(crate) fn geometry_of(sectors: &IbmSectors) -> Result<IbmGeometry> {
    let claims = &sectors.inspect().claims;
    if claims.is_empty() {
        return Err(refuse(
            "this recording states no records, so there is no geometry to \
             compose an extent from",
        ));
    }

    // One data-field size. A recording mixing sizes has no single
    // block, and every layer above is written against one.
    let size_code = claims[0].size_code;
    if let Some(other) = claims.iter().find(|claim| claim.size_code != size_code) {
        return Err(refuse(format!(
            "this recording states more than one data-field size — cylinder {} \
             head {} sector {} states size code {} where cylinder {} head {} \
             sector {} states {}; a linear extent has one block size, and \
             flattening these would put every file's contents somewhere other \
             than where they are",
            claims[0].cylinder,
            claims[0].head,
            claims[0].sector,
            size_code,
            other.cylinder,
            other.head,
            other.sector,
            other.size_code
        )));
    }
    let sector_bytes = 128u32.checked_shl(u32::from(size_code)).ok_or_else(|| {
        refuse(format!(
            "size code {size_code} states no data field this large"
        ))
    })?;

    // The span the records describe, counted from zero: a recording
    // that names cylinder 39 describes forty of them whether or not it
    // states every one.
    let cylinders = u32::from(claims.iter().map(|claim| claim.cylinder).max().unwrap_or(0)) + 1;
    let heads = u32::from(claims.iter().map(|claim| claim.head).max().unwrap_or(0)) + 1;

    // One contiguous run of sector numbers. The lowest is read rather
    // than assumed to be one, because numbering from one is a
    // convention and this reads what is there.
    let first_sector = claims.iter().map(|claim| claim.sector).min().unwrap_or(0);
    let last_sector = claims.iter().map(|claim| claim.sector).max().unwrap_or(0);
    let sectors_per_track = u32::from(last_sector - first_sector) + 1;

    let mut seen = vec![false; sectors_per_track as usize];
    for claim in claims {
        let index = (claim.sector - first_sector) as usize;
        seen[index] = true;
    }
    if let Some(missing) = seen.iter().position(|found| !found) {
        return Err(refuse(format!(
            "this recording's sector numbers run from {first_sector} to \
             {last_sector} and state none numbered {}; a linear extent needs a \
             contiguous run, and inventing the gap would shift every record \
             after it",
            first_sector as u32 + missing as u32
        )));
    }

    Ok(IbmGeometry {
        cylinders,
        heads,
        sectors_per_track,
        first_sector,
        sector_bytes,
    })
}

/// A recording's records presented as the linear extent above them
/// (D62).
///
/// It is read-only. Lowering a byte back into a record is the write path
/// F69 and F70 own, and until one exists a write here is refused rather
/// than silently dropped.
pub(crate) struct IbmExtent<'a> {
    sectors: &'a IbmSectors,
    geometry: IbmGeometry,
}

impl<'a> IbmExtent<'a> {
    pub(crate) fn new(sectors: &'a IbmSectors) -> Result<Self> {
        let geometry = geometry_of(sectors)?;
        Ok(Self { sectors, geometry })
    }

    pub(crate) fn geometry(&self) -> IbmGeometry {
        self.geometry
    }
}

impl Device for IbmExtent<'_> {
    fn len(&self) -> u64 {
        self.geometry.length_bytes()
    }

    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<()> {
        let mut filled = 0usize;
        while filled < buf.len() {
            let at = offset + filled as u64;
            let Some((cylinder, head, sector, within)) = self.geometry.record_at(at) else {
                return Err(refuse(format!(
                    "this recording composes {} bytes and the read reaches {}",
                    self.geometry.length_bytes(),
                    at
                )));
            };
            // A hole refuses the read that touches it and no other. The
            // sector layer's own refusal is the one that travels,
            // because it carries both checksums and this seam has
            // nothing truer to say.
            let payload = self.sectors.read_sector(cylinder, head, sector)?;
            let take = (payload.len() - within).min(buf.len() - filled);
            buf[filled..filled + take].copy_from_slice(&payload[within..within + take]);
            filled += take;
        }
        Ok(())
    }

    fn write_at(&mut self, _offset: u64, _data: &[u8]) -> Result<()> {
        Err(Error::categorized_image(
            ErrorCategory::ReadOnly,
            "ibm",
            "this extent is composed over a recording's own records, and \
             lowering a byte back into a record is not delivered: a write here \
             would have to decide where the cell boundaries move, which is the \
             recording's question and not this seam's",
        ))
    }

    fn flush(&mut self) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn geometry() -> IbmGeometry {
        IbmGeometry {
            cylinders: 40,
            heads: 2,
            sectors_per_track: 9,
            first_sector: 1,
            sector_bytes: 512,
        }
    }

    #[test]
    fn every_record_addresses_the_place_the_extent_reads_it_back_from() {
        let geometry = geometry();
        let mut seen = std::collections::BTreeSet::new();
        for cylinder in 0..40u8 {
            for head in 0..2u8 {
                for sector in 1..=9u8 {
                    let at = geometry
                        .offset_of(cylinder, head, sector)
                        .expect("the record is within the geometry");
                    // The two directions are inverses. A wrong ordering
                    // is exactly what this catches, and it is the kind
                    // of wrong that still lists a directory and only
                    // corrupts what it serves.
                    assert_eq!(
                        geometry.record_at(at),
                        Some((cylinder, head, sector, 0)),
                        "cylinder {cylinder} head {head} sector {sector}"
                    );
                    assert!(seen.insert(at), "two records share offset {at}");
                }
            }
        }
        // And together they cover the extent exactly — no gap, no
        // overlap, nothing past the end.
        assert_eq!(seen.len() as u64, geometry.length_bytes() / 512);
        assert_eq!(
            seen.iter().max().copied(),
            Some(geometry.length_bytes() - 512)
        );
    }

    #[test]
    fn an_address_outside_the_geometry_is_no_place_at_all() {
        let geometry = geometry();
        assert_eq!(geometry.offset_of(40, 0, 1), None, "past the last cylinder");
        assert_eq!(geometry.offset_of(0, 2, 1), None, "past the last head");
        assert_eq!(geometry.offset_of(0, 0, 10), None, "past the last sector");
        assert_eq!(geometry.offset_of(0, 0, 0), None, "below the first sector");
        assert_eq!(
            geometry.record_at(geometry.length_bytes()),
            None,
            "past the end of the extent"
        );
    }

    #[test]
    fn a_recording_numbered_from_zero_is_read_as_it_is_written() {
        // Numbering from one is a convention, not a rule, and the
        // geometry reads what is there.
        let geometry = IbmGeometry {
            first_sector: 0,
            ..geometry()
        };
        assert_eq!(geometry.offset_of(0, 0, 0), Some(0));
        assert_eq!(geometry.record_at(0), Some((0, 0, 0, 0)));
        assert_eq!(geometry.offset_of(0, 0, 9), None, "nine is the tenth");
    }
}
