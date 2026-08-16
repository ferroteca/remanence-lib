// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The ImageDisk reader (F68, P12, P13, P27, P28), and the sector
//! ordering it resolves (D60).
//!
//! **An ImageDisk track states its sectors' identities, and this adapter
//! applies them.** The records inside a track sit in the physical order
//! they were recorded, and a separate map says which id each one carries.
//! A raw dump of the same disk is already in id order, its physical
//! interleave flattened by whoever dumped it — so the same recording
//! arrives in two orders depending on the container, and something has to
//! resolve that.
//!
//! D60 puts the resolution here, because the evidence is here. The id map
//! is in this file and nowhere else, so a layer above resolving the order
//! would be applying a rule it cannot check. The rule's other half
//! matters as much: **nothing is resolved that the format does not
//! state.** A raw dump says nothing about interleave, so nothing is
//! resolved for one and whatever remains is a declaration some layer
//! above makes and answers for.
//!
//! The Heath CP/M disks are the worked example. Their hard-sectored raw
//! dumps need a four-way skew declared in the CP/M layout, the interleave
//! having lived in the drive's BIOS. Their soft-sectored ImageDisk images
//! of the same release need none: the interleave is in the sector
//! numbering, and this adapter applies it.
//!
//! **With ids resolved, a uniform image is a linear extent**, which is
//! why nothing about the device seam changes for it. A non-uniform one is
//! a linear extent too — the tracks laid end to end — but it has no
//! single coordinate system, so it declares no geometry and the sector
//! verbs refuse through the geometry seam's own rule rather than
//! addressing it wrongly.
//!
//! **A sector the artifact records as unrecovered is not zeroes.** Its
//! extent is held as a hole and a read touching one is refused with its
//! range. That is content rather than a shortfall of the file: the
//! artifact is exactly what it claims to be, and part of what it claims
//! is that those sectors were never read.

use crate::error::{Error, ErrorCategory, Result};
use crate::image::adapters::DiskDescriptor;
use crate::io::device::{Device, MediumDevice};

/// Every ImageDisk file opens with this, then a version and a timestamp.
pub(crate) const IMD_MAGIC: &[u8] = b"IMD ";

/// The byte ending the ASCII header, after which the track records begin.
const HEADER_END: u8 = 0x1a;

/// The largest artifact this reader will decode whole (P27).
///
/// ImageDisk is a floppy format; the bound is generous for one and is
/// stated rather than assumed, as the namespace readers' bounds are.
const IMD_BOUND: u64 = 8 * 1024 * 1024;

fn refuse(reason: impl Into<String>) -> Error {
    Error::invalid_image("imd", reason)
}

/// What one track's encoding byte says, in the format's own terms.
fn mode_name(mode: u8) -> Option<&'static str> {
    Some(match mode {
        0 => "FM, 500 kbps",
        1 => "FM, 300 kbps",
        2 => "FM, 250 kbps",
        3 => "MFM, 500 kbps",
        4 => "MFM, 300 kbps",
        5 => "MFM, 250 kbps",
        _ => return None,
    })
}

/// One sector's record type, split into the three facts it crosses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RecordKind {
    /// The artifact holds no data for this sector at all.
    unavailable: bool,
    /// Stored as one repeated fill byte rather than literally.
    compressed: bool,
    /// The address mark was a deleted-data mark.
    deleted: bool,
    /// The recovery reported an error over the data.
    error: bool,
}

impl RecordKind {
    /// The ten values the format defines, or the refusal naming the one
    /// it does not.
    fn read(byte: u8) -> Result<Self> {
        if byte > 8 {
            return Err(refuse(format!(
                "a sector record opens with the type byte {byte}, and ImageDisk defines \
                 0 through 8; nothing here reads a record whose encoding the format does \
                 not state"
            )));
        }
        if byte == 0 {
            return Ok(Self {
                unavailable: true,
                compressed: false,
                deleted: false,
                error: false,
            });
        }
        // 1..=8 cross three flags: the low bit distinguishes literal from
        // compressed, and the pairs above it add the deleted mark and the
        // read error in turn.
        let at = byte - 1;
        Ok(Self {
            unavailable: false,
            compressed: at % 2 == 1,
            deleted: (at / 2) % 2 == 1,
            error: at >= 4,
        })
    }
}

/// What a decoded ImageDisk holds, before it becomes a device.
#[derive(Debug)]
pub(crate) struct ImdContent {
    /// The sectors of every track, in cylinder, head and stated-id
    /// order — the linear extent everything above reads.
    pub(crate) data: Vec<u8>,
    /// The extents of sectors the artifact records as never recovered,
    /// in ascending order. Reads touching one are refused.
    pub(crate) holes: Vec<(u64, u64)>,
    /// The coordinates this recording establishes, where it is uniform
    /// enough to have any.
    pub(crate) geometry: Option<DiskDescriptor>,
    /// What the decode observed, for the load's account (P4).
    pub(crate) evidence: Vec<String>,
}

/// One track, as the file states it.
struct Track {
    mode: u8,
    cylinder: u8,
    head: u8,
    sector_bytes: u32,
    /// The stated id of each sector, in the physical order stored.
    ids: Vec<u8>,
    /// Each sector's payload in that same physical order, and whether
    /// the artifact holds it at all.
    records: Vec<(RecordKind, Vec<u8>)>,
}

/// Reads a whole ImageDisk artifact.
pub(crate) fn decode(bytes: &[u8]) -> Result<ImdContent> {
    if !bytes.starts_with(IMD_MAGIC) {
        return Err(refuse(
            "the artifact does not open with ImageDisk's own signature",
        ));
    }
    let header_end = bytes
        .iter()
        .position(|byte| *byte == HEADER_END)
        .ok_or_else(|| {
            refuse(
                "the ASCII header is never terminated, so the track records have no \
                 stated beginning",
            )
        })?;
    let comment = String::from_utf8_lossy(&bytes[..header_end])
        .trim_end()
        .to_owned();

    let mut at = header_end + 1;
    let mut tracks: Vec<Track> = Vec::new();
    while at < bytes.len() {
        let track = read_track(bytes, &mut at, tracks.len())?;
        tracks.push(track);
    }
    if tracks.is_empty() {
        return Err(refuse(
            "the artifact states no tracks at all, so there is no recording to read",
        ));
    }

    assemble(tracks, comment)
}

/// Reads one track record, advancing `at` past it.
fn read_track(bytes: &[u8], at: &mut usize, ordinal: usize) -> Result<Track> {
    let need = |at: usize, count: usize, what: &str| -> Result<()> {
        if at + count > bytes.len() {
            return Err(refuse(format!(
                "track record {ordinal} runs past the end of the artifact while reading \
                 {what}: the file is short of what its own structure states"
            )));
        }
        Ok(())
    };

    need(*at, 5, "its header")?;
    let mode = bytes[*at];
    let cylinder = bytes[*at + 1];
    let head_byte = bytes[*at + 2];
    let sectors = bytes[*at + 3] as usize;
    let size_code = bytes[*at + 4];
    *at += 5;

    if mode_name(mode).is_none() {
        return Err(refuse(format!(
            "track record {ordinal} states the encoding mode {mode}, and ImageDisk \
             defines 0 through 5"
        )));
    }
    if size_code > 6 {
        return Err(refuse(format!(
            "track record {ordinal} states the sector-size code {size_code}, and \
             ImageDisk defines 0 through 6"
        )));
    }
    let sector_bytes = 128u32 << size_code;
    if sectors == 0 {
        return Err(refuse(format!(
            "track record {ordinal} states no sectors; an unformatted track is recorded \
             by omitting it, not by declaring an empty one"
        )));
    }

    need(*at, sectors, "its sector-id map")?;
    let ids = bytes[*at..*at + sectors].to_vec();
    *at += sectors;

    // The two optional maps, each present where the head byte says so.
    let has_cylinder_map = head_byte & 0x80 != 0;
    let has_head_map = head_byte & 0x40 != 0;
    let head = head_byte & 0x3f;

    if has_cylinder_map {
        need(*at, sectors, "its cylinder map")?;
        let map = &bytes[*at..*at + sectors];
        if let Some(found) = map.iter().find(|stated| **stated != cylinder) {
            return Err(refuse(format!(
                "track record {ordinal} declares cylinder {cylinder} and its cylinder \
                 map states {found} for one of its sectors; the two readings disagree \
                 and neither is preferred"
            )));
        }
        *at += sectors;
    }
    if has_head_map {
        need(*at, sectors, "its head map")?;
        let map = &bytes[*at..*at + sectors];
        if let Some(found) = map.iter().find(|stated| **stated != head) {
            return Err(refuse(format!(
                "track record {ordinal} declares head {head} and its head map states \
                 {found} for one of its sectors; the two readings disagree and neither \
                 is preferred"
            )));
        }
        *at += sectors;
    }

    let mut sorted = ids.clone();
    sorted.sort_unstable();
    let duplicates = sorted.windows(2).any(|pair| pair[0] == pair[1]);
    if duplicates {
        return Err(refuse(format!(
            "track record {ordinal} states the same sector id twice, so ordering its \
             sectors by the identity they claim has no single answer"
        )));
    }

    let mut records = Vec::with_capacity(sectors);
    for slot in 0..sectors {
        need(*at, 1, "a sector record's type")?;
        let kind = RecordKind::read(bytes[*at])?;
        *at += 1;
        if kind.unavailable {
            records.push((kind, Vec::new()));
            continue;
        }
        if kind.compressed {
            need(*at, 1, "a compressed sector's fill byte")?;
            let fill = bytes[*at];
            *at += 1;
            records.push((kind, vec![fill; sector_bytes as usize]));
        } else {
            need(*at, sector_bytes as usize, "a sector's data")?;
            records.push((kind, bytes[*at..*at + sector_bytes as usize].to_vec()));
            *at += sector_bytes as usize;
        }
        let _ = slot;
    }

    Ok(Track {
        mode,
        cylinder,
        head,
        sector_bytes,
        ids,
        records,
    })
}

/// Lays the decoded tracks out in the order the recording numbers them.
fn assemble(mut tracks: Vec<Track>, comment: String) -> Result<ImdContent> {
    // Cylinder then head is the order a recording is read in, and the
    // artifact is free to state its tracks in any order at all.
    tracks.sort_by_key(|track| (track.cylinder, track.head));

    let mut data = Vec::new();
    let mut holes: Vec<(u64, u64)> = Vec::new();
    let mut unavailable = 0u64;
    let mut deleted = 0u64;
    let mut errored = 0u64;
    let mut compressed = 0u64;

    for track in &tracks {
        // The ids sorted ascending are the order this lays sectors in:
        // the identity the recording claims, not the position it sat at.
        let mut order: Vec<usize> = (0..track.ids.len()).collect();
        order.sort_by_key(|slot| track.ids[*slot]);

        for slot in order {
            let (kind, payload) = &track.records[slot];
            let start = data.len() as u64;
            if kind.unavailable {
                unavailable += 1;
                data.resize(data.len() + track.sector_bytes as usize, 0);
                holes.push((start, start + u64::from(track.sector_bytes)));
            } else {
                data.extend_from_slice(payload);
            }
            if kind.deleted {
                deleted += 1;
            }
            if kind.error {
                errored += 1;
            }
            if kind.compressed {
                compressed += 1;
            }
        }
    }

    let geometry = uniform_geometry(&tracks);
    let mut evidence = vec![format!(
        "ImageDisk: {} track(s), laid out in the sector order the recording states \
         rather than the order the artifact stores them (D60)",
        tracks.len()
    )];
    // The header is whatever the imaging tool wrote there, and on a real
    // artifact it is often the only record of what was imaged — a part
    // number, a serial, which disk of a set. It is carried as the free
    // text it is, never parsed for facts.
    if !comment.is_empty() {
        for line in comment.lines().filter(|line| !line.trim().is_empty()) {
            evidence.push(format!("the artifact's own header states: {}", line.trim()));
        }
    }
    let modes: Vec<&str> = {
        let mut seen: Vec<u8> = tracks.iter().map(|track| track.mode).collect();
        seen.sort_unstable();
        seen.dedup();
        seen.iter()
            .filter_map(|mode| mode_name(*mode))
            .collect::<Vec<_>>()
    };
    evidence.push(format!("recorded {}", modes.join(" and ")));
    match &geometry {
        Some(disk) => evidence.push(format!(
            "every track holds {} sectors of {} bytes, so the recording establishes \
             {} cylinder(s) of {} head(s)",
            disk.sectors_per_track, disk.sector_size, disk.cylinders, disk.sides
        )),
        None => evidence.push(
            "the tracks do not all hold the same sectors of the same size, so this \
             recording has no single set of coordinates and declares none; its bytes \
             and the filesystems above them read, and the sector verbs refuse rather \
             than addressing it by a geometry it does not have"
                .to_owned(),
        ),
    }
    if compressed > 0 {
        evidence.push(format!(
            "{compressed} sector(s) were stored as one repeated byte rather than \
             literally, which is an encoding of the artifact and not of the recording"
        ));
    }
    if deleted > 0 {
        evidence.push(format!(
            "{deleted} sector(s) were recorded behind a deleted-data address mark; \
             their bytes are served as the artifact holds them"
        ));
    }
    if errored > 0 {
        evidence.push(format!(
            "{errored} sector(s) were recovered with a reported data error; their bytes \
             are served as the artifact holds them"
        ));
    }
    if unavailable > 0 {
        evidence.push(format!(
            "{unavailable} sector(s) were never recovered and the artifact says so; \
             their extents hold nothing, and a read touching one is refused rather than \
             answered with zeroes"
        ));
    }

    Ok(ImdContent {
        data,
        holes,
        geometry,
        evidence,
    })
}

/// The coordinates a recording establishes, where every track agrees on
/// what a track is.
///
/// Disagreement is not a conflict to resolve: the recording simply is not
/// uniform, and saying so is the honest answer (F80 is what will give
/// such a recording coordinates of its own).
fn uniform_geometry(tracks: &[Track]) -> Option<DiskDescriptor> {
    let first = tracks.first()?;
    let sectors = first.ids.len() as u32;
    let sector_size = first.sector_bytes;
    if tracks
        .iter()
        .any(|track| track.ids.len() as u32 != sectors || track.sector_bytes != sector_size)
    {
        return None;
    }

    let mut heads: Vec<u8> = tracks.iter().map(|track| track.head).collect();
    heads.sort_unstable();
    heads.dedup();
    let mut cylinders: Vec<u8> = tracks.iter().map(|track| track.cylinder).collect();
    cylinders.sort_unstable();
    cylinders.dedup();

    // Every cylinder must carry every head, or the tracks do not form a
    // rectangle and there is no tuple describing them either.
    if cylinders.len() * heads.len() != tracks.len() {
        return None;
    }

    Some(DiskDescriptor {
        sector_size: u64::from(sector_size),
        cylinders: cylinders.len() as u32,
        sides: heads.len() as u32,
        sectors_per_track: sectors,
    })
}

/// The linear extent a decoded ImageDisk's sectors make in stated-id
/// order, with the holes its unrecovered sectors leave.
///
/// It is separate from the artifact's own handle because it is the whole
/// of what reads: the host is held for the medium's sake, and nothing
/// about serving a byte needs it.
#[derive(Debug)]
pub(crate) struct ImdExtent {
    data: Vec<u8>,
    holes: Vec<(u64, u64)>,
}

impl ImdExtent {
    pub(crate) fn new(content: &ImdContent) -> Self {
        Self {
            data: content.data.clone(),
            holes: content.holes.clone(),
        }
    }

    /// Refuses a read that would touch a sector the artifact never held.
    fn check(&self, offset: u64, length: u64) -> Result<()> {
        let end = offset.saturating_add(length);
        for (start, stop) in &self.holes {
            if offset < *stop && *start < end {
                return Err(Error::categorized_image(
                    ErrorCategory::Unavailable,
                    "imd",
                    format!(
                        "bytes {offset}..{end} reach {start}..{stop}, which the artifact \
                         records as a sector that was never recovered: the recording \
                         holds nothing there, and zeroes would be an answer nobody read"
                    ),
                ));
            }
        }
        Ok(())
    }

    pub(crate) fn bound(len: u64) -> Result<()> {
        if len > IMD_BOUND {
            return Err(Error::categorized_image(
                ErrorCategory::Unsupported,
                "imd",
                format!(
                    "this artifact is {len} bytes and the ImageDisk reader is bounded to \
                     {IMD_BOUND}"
                ),
            ));
        }
        Ok(())
    }
}

/// A decoded ImageDisk beside the handle it was read through.
#[derive(Debug)]
pub(crate) struct ImdImage {
    host: MediumDevice,
    extent: ImdExtent,
    geometry: Option<DiskDescriptor>,
    evidence: Vec<String>,
}

impl ImdImage {
    pub(crate) fn new(host: MediumDevice, content: &ImdContent) -> Self {
        Self {
            host,
            extent: ImdExtent::new(content),
            geometry: content.geometry,
            evidence: content.evidence.clone(),
        }
    }

    pub(crate) fn host_mut(&mut self) -> &mut MediumDevice {
        &mut self.host
    }

    pub(crate) fn geometry(&self) -> Option<DiskDescriptor> {
        self.geometry
    }

    /// What the decode observed, for the load's account (P4).
    pub(crate) fn evidence(&self) -> &[String] {
        &self.evidence
    }
}

impl Device for ImdImage {
    fn len(&self) -> u64 {
        self.extent.len()
    }

    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<()> {
        self.extent.read_at(offset, buf)
    }

    fn write_at(&mut self, offset: u64, data: &[u8]) -> Result<()> {
        self.extent.write_at(offset, data)
    }

    fn flush(&mut self) -> Result<()> {
        Ok(())
    }
}

impl Device for ImdExtent {
    fn len(&self) -> u64 {
        self.data.len() as u64
    }

    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<()> {
        self.check(offset, buf.len() as u64)?;
        let start = offset as usize;
        let end = start
            .checked_add(buf.len())
            .filter(|end| *end <= self.data.len())
            .ok_or_else(|| {
                Error::categorized_image(
                    ErrorCategory::Unavailable,
                    "imd",
                    format!(
                        "bytes {offset}..{} are past the {} this recording holds",
                        offset + buf.len() as u64,
                        self.data.len()
                    ),
                )
            })?;
        buf.copy_from_slice(&self.data[start..end]);
        Ok(())
    }

    fn write_at(&mut self, _offset: u64, _data: &[u8]) -> Result<()> {
        Err(Error::categorized_image(
            ErrorCategory::Unsupported,
            "imd",
            "this release reads ImageDisk and does not write it: a record's encoded \
             length changes with its contents, so a write relocates every record after \
             it and re-encodes the container",
        ))
    }

    fn flush(&mut self) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds an ImageDisk artifact: one entry per track, each stating
    /// its sector ids in the physical order they are stored.
    fn build(tracks: &[(u8, u8, u32, Vec<u8>)]) -> Vec<u8> {
        let mut out = b"IMD 1.18: test\x1a".to_vec();
        for (cylinder, head, sector_bytes, ids) in tracks {
            let code = (sector_bytes.trailing_zeros() - 7) as u8;
            out.extend_from_slice(&[1, *cylinder, *head, ids.len() as u8, code]);
            out.extend_from_slice(ids);
            for id in ids {
                // Each sector is a compressed record of its own id, so
                // the content says which sector it is.
                out.push(2);
                out.push(*id);
            }
        }
        out
    }

    #[test]
    fn sectors_are_laid_out_by_the_identity_the_recording_states() {
        // The physical order is the interleave; the ids are 1..=5. What
        // comes back must be in id order, which is D60's whole content.
        let artifact = build(&[(0, 0, 128, vec![1, 4, 2, 5, 3])]);
        let content = decode(&artifact).expect("the artifact decodes");
        assert_eq!(content.data.len(), 5 * 128);
        let first_of_each: Vec<u8> = content
            .data
            .chunks_exact(128)
            .map(|sector| sector[0])
            .collect();
        assert_eq!(
            first_of_each,
            [1, 2, 3, 4, 5],
            "each sector's payload is its own id, so id order is what this reads"
        );
    }

    #[test]
    fn a_uniform_recording_establishes_coordinates() {
        let artifact = build(&[
            (0, 0, 256, (1..=10).collect()),
            (1, 0, 256, (1..=10).collect()),
        ]);
        let content = decode(&artifact).expect("the artifact decodes");
        let geometry = content
            .geometry
            .expect("a uniform recording has coordinates");
        assert_eq!(geometry.cylinders, 2);
        assert_eq!(geometry.sides, 1);
        assert_eq!(geometry.sectors_per_track, 10);
        assert_eq!(geometry.sector_size, 256);
    }

    #[test]
    fn a_recording_whose_tracks_differ_declares_no_coordinates() {
        // The ordinary CP/M and DOS floppy: track 0 is not like the rest.
        // There is no tuple describing it, and inventing one would
        // mis-address every track it does not fit.
        let artifact = build(&[
            (0, 0, 128, (1..=8).collect()),
            (1, 0, 256, (1..=10).collect()),
        ]);
        let content = decode(&artifact).expect("the artifact decodes");
        assert!(
            content.geometry.is_none(),
            "no single set of coordinates covers it"
        );
        assert!(
            content
                .evidence
                .iter()
                .any(|line| line.contains("no single set of coordinates")),
            "and the account says so: {:?}",
            content.evidence
        );
        // The bytes are still all there — this is a refusal to address,
        // not a refusal to read.
        assert_eq!(content.data.len(), 8 * 128 + 10 * 256);
    }

    #[test]
    fn an_unrecovered_sector_is_a_hole_and_not_zeroes() {
        let mut artifact = b"IMD 1.18: test\x1a".to_vec();
        artifact.extend_from_slice(&[1, 0, 0, 3, 0]);
        artifact.extend_from_slice(&[1, 2, 3]);
        artifact.extend_from_slice(&[2, 0xaa]); // sector 1, compressed
        artifact.push(0); // sector 2, never recovered
        artifact.extend_from_slice(&[2, 0xcc]); // sector 3, compressed
        let content = decode(&artifact).expect("the artifact decodes");
        assert_eq!(content.holes, vec![(128, 256)]);

        let mut image = ImdExtent::new(&content);
        let mut buf = [0u8; 128];
        image.read_at(0, &mut buf).expect("sector 1 reads");
        assert_eq!(buf[0], 0xaa);
        let error = image
            .read_at(128, &mut buf)
            .expect_err("the unrecovered sector is refused");
        assert_eq!(error.category(), ErrorCategory::Unavailable);
        image.read_at(256, &mut buf).expect("sector 3 reads");
        assert_eq!(buf[0], 0xcc);
    }

    #[test]
    fn a_duplicate_sector_id_is_refused_rather_than_ordered_arbitrarily() {
        let artifact = build(&[(0, 0, 128, vec![1, 2, 2, 4])]);
        let error = decode(&artifact).expect_err("two sectors claim one identity");
        assert!(
            error.to_string().contains("same sector id twice"),
            "{error}"
        );
    }

    #[test]
    fn a_type_byte_the_format_does_not_define_is_refused() {
        let mut artifact = b"IMD 1.18: test\x1a".to_vec();
        artifact.extend_from_slice(&[1, 0, 0, 1, 0]);
        artifact.push(1);
        artifact.push(9);
        let error = decode(&artifact).expect_err("ImageDisk defines 0 through 8");
        assert!(error.to_string().contains("type byte 9"), "{error}");
    }

    #[test]
    fn a_cylinder_map_disagreeing_with_its_track_is_refused() {
        let mut artifact = b"IMD 1.18: test\x1a".to_vec();
        // Head byte 0x80 declares a cylinder map follows.
        artifact.extend_from_slice(&[1, 7, 0x80, 2, 0]);
        artifact.extend_from_slice(&[1, 2]);
        artifact.extend_from_slice(&[7, 9]); // the map disagrees on one
        artifact.extend_from_slice(&[2, 0, 2, 0]);
        let error = decode(&artifact).expect_err("the two readings disagree");
        assert!(error.to_string().contains("cylinder map"), "{error}");
    }

    #[test]
    fn a_track_running_past_the_artifact_is_refused() {
        let mut artifact = build(&[(0, 0, 256, (1..=10).collect())]);
        artifact.truncate(artifact.len() - 6);
        let error = decode(&artifact).expect_err("the file is short of its own structure");
        assert!(error.to_string().contains("runs past the end"), "{error}");
    }

    #[test]
    fn tracks_are_read_in_coordinate_order_whatever_order_they_are_stored_in() {
        let artifact = build(&[(1, 0, 128, vec![1]), (0, 0, 128, vec![2])]);
        let content = decode(&artifact).expect("the artifact decodes");
        // Cylinder 0 comes first in the extent even though it is second
        // in the file, and its sector's payload is its own id.
        assert_eq!(content.data[0], 2);
        assert_eq!(content.data[128], 1);
    }
}
