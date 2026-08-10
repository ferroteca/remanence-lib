// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The C64 renditions (F66): d64, g64 and p64 mastered off a
//! remanence-backed image — P29 acts where only the destination
//! varies, each stating its loss.
//!
//! The GCR machinery here — the 4-to-5 group code, the sync scan, the
//! header and data blocks with their checksums — is crate-private
//! analysis serving the reconstruction and these renditions. It is
//! deliberately not the F61 sector surface, which remains the
//! user-facing rung in the media-first shape. Nothing is repaired and
//! nothing is rejected: a failed checksum is recorded, and what enters
//! a rendition is decided by the rendition's own rules.
#![allow(dead_code)]

use std::path::Path;

use crate::error::{Error, Result};
use crate::evidence::Provenance;
use crate::flux_analysis::cell_of;
use crate::remanence_image::{ANGULAR_DIVISIONS, Orbit, REMANENCE, RemanenceImage};
use crate::remanence_reconstruction::RemanenceDisk;

/// The 1541's reference frame, as the delivered P64 adapter declares
/// it: 16 MHz over 300 rpm.
const CYCLES_PER_ROTATION: u64 = 3_200_000;

/// The rig lattice the image's radii resolve against, shared with the
/// reconstruction.
const FIRST_STEP_RADIUS_MICRONS: i64 = 57_150;

/// The grid step an orbit's radius sits on, or `None` for an orbit
/// deliberately off the 96 tpi grid — refused rather than pulled onto
/// a neighbour. The nearest thing it must reject is a half-track 132
/// microns away, and a whole-micron radius cannot divide the exact
/// pitch, so the confirmation allows one micron of rounding.
pub(crate) fn grid_step_of(radius_microns: u64) -> Option<u64> {
    let from_anchor = FIRST_STEP_RADIUS_MICRONS - radius_microns as i64;
    let step = (from_anchor as f64 * 12.0 / 3175.0).round() as i64;
    if !(0..=200).contains(&step) {
        return None;
    }
    let on_grid = (FIRST_STEP_RADIUS_MICRONS as f64 - step as f64 * 3175.0 / 12.0).round() as i64;
    ((on_grid - radius_microns as i64).abs() <= 1).then_some(step as u64)
}

/// Channel bits appended in read order, most significant bit of each
/// byte first.
pub(crate) struct Bitstream {
    bytes: Vec<u8>,
    bits: u64,
}

impl Bitstream {
    pub(crate) fn new() -> Self {
        Self {
            bytes: Vec::new(),
            bits: 0,
        }
    }

    pub(crate) fn len(&self) -> u64 {
        self.bits
    }

    pub(crate) fn append_one(&mut self) {
        let at = self.bits as usize;
        if at / 8 >= self.bytes.len() {
            self.bytes.push(0);
        }
        self.bytes[at / 8] |= 0x80 >> (at % 8);
        self.bits += 1;
    }

    pub(crate) fn append_zeros(&mut self, count: u64) {
        self.bits += count;
        let needed = (self.bits as usize).div_ceil(8);
        if needed > self.bytes.len() {
            self.bytes.resize(needed, 0);
        }
    }

    /// The packed bytes, most significant bit first.
    pub(crate) fn into_bytes(mut self) -> Vec<u8> {
        let needed = (self.bits as usize).div_ceil(8);
        self.bytes.resize(needed, 0);
        self.bytes
    }
}

/// Clocks coherent transition angles into channel bits by the
/// phase-locked half-window: each interval spans a whole number of
/// cells, nearest wins, and the final interval wraps past the origin
/// to the first transition so the loop closes with no seam.
pub(crate) fn clock(angles: &[i64], cell_divisions: f64) -> Bitstream {
    let divisions = ANGULAR_DIVISIONS as i64;
    let mut stream = Bitstream::new();
    for i in 0..angles.len() {
        let from = angles[i];
        let to = angles[(i + 1) % angles.len()];
        let mut interval = to - from;
        if interval <= 0 {
            interval += divisions;
        }
        let cells = ((interval as f64 / cell_divisions).round() as u64).max(1);
        stream.append_zeros(cells - 1);
        stream.append_one();
    }
    stream
}

/// The 1541's 4-to-5 group code.
mod gcr {
    const ENCODE: [u8; 16] = [
        0b01010, 0b01011, 0b10010, 0b10011, 0b01110, 0b01111, 0b10110, 0b10111, 0b01001,
        0b11001, 0b11010, 0b11011, 0b01101, 0b11101, 0b11110, 0b10101,
    ];

    /// The nibble a five-bit group encodes, or `None` for a group the
    /// table does not assign.
    pub(crate) fn decode_group(group: u8) -> Option<u8> {
        ENCODE.iter().position(|&code| code == group & 0x1f).map(|nibble| nibble as u8)
    }

    pub(crate) fn encode_nibble(nibble: u8) -> u8 {
        ENCODE[usize::from(nibble & 0x0f)]
    }
}

/// The recorded blocks of one clocked track: headers, data blocks, and
/// the pairing the medium lays out — header, gap, sync, data. Nothing
/// is repaired and nothing is rejected.
pub(crate) struct GcrTrack {
    bits: Vec<u8>,
    bit_length: usize,
}

pub(crate) const HEADER_MARK: u8 = 0x08;
pub(crate) const DATA_MARK: u8 = 0x07;
const HEADER_BYTES: usize = 8;
const DATA_BYTES: usize = 260;
const SYNC_BITS: usize = 10;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GcrHeader {
    pub(crate) at_bit: usize,
    pub(crate) track: u8,
    pub(crate) sector: u8,
    pub(crate) id: (u8, u8),
    pub(crate) stored_checksum: u8,
    pub(crate) computed_checksum: u8,
    pub(crate) illegal_groups: bool,
}

impl GcrHeader {
    pub(crate) fn checksum_ok(&self) -> bool {
        !self.illegal_groups && self.stored_checksum == self.computed_checksum
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GcrData {
    pub(crate) at_bit: usize,
    pub(crate) bytes: Vec<u8>,
    pub(crate) stored_checksum: u8,
    pub(crate) computed_checksum: u8,
    pub(crate) illegal_groups: bool,
}

impl GcrData {
    pub(crate) fn checksum_ok(&self) -> bool {
        !self.illegal_groups && self.stored_checksum == self.computed_checksum
    }
}

#[derive(Debug, Clone)]
pub(crate) struct GcrSector {
    pub(crate) header: GcrHeader,
    pub(crate) data: Option<GcrData>,
}

impl GcrTrack {
    pub(crate) fn new(bits: Vec<u8>, bit_length: usize) -> Self {
        Self { bits, bit_length }
    }

    fn bit(&self, index: usize) -> bool {
        let at = index % self.bit_length;
        self.bits[at >> 3] & (0x80 >> (at & 7)) != 0
    }

    fn read_bits(&self, at: usize, count: usize) -> u8 {
        let mut value = 0u8;
        for i in 0..count {
            value = (value << 1) | u8::from(self.bit(at + i));
        }
        value
    }

    fn read_block(&self, at: usize, out: &mut [u8]) -> bool {
        let mut illegal = false;
        let mut bit = at;
        for slot in out.iter_mut() {
            let high = gcr::decode_group(self.read_bits(bit, 5));
            let low = gcr::decode_group(self.read_bits(bit + 5, 5));
            bit += 10;
            if high.is_none() || low.is_none() {
                illegal = true;
            }
            *slot = (high.unwrap_or(0) << 4) | low.unwrap_or(0);
        }
        illegal
    }

    /// Every position where a sync run of at least ten ones ends. Two
    /// passes' worth of scanning, so a sync spanning the origin is
    /// seen; positions are recorded modulo the track.
    pub(crate) fn block_starts(&self) -> Vec<usize> {
        let mut starts = Vec::new();
        let mut run = 0usize;
        for i in 0..self.bit_length * 2 {
            if self.bit(i) {
                run += 1;
            } else {
                if run >= SYNC_BITS {
                    let at = i % self.bit_length;
                    if !starts.contains(&at) {
                        starts.push(at);
                    }
                }
                run = 0;
            }
        }
        starts
    }

    fn read_header(&self, at: usize) -> GcrHeader {
        let mut out = [0u8; HEADER_BYTES];
        let illegal = self.read_block(at, &mut out);
        GcrHeader {
            at_bit: at,
            track: out[3],
            sector: out[2],
            id: (out[5], out[4]),
            stored_checksum: out[1],
            computed_checksum: out[2] ^ out[3] ^ out[4] ^ out[5],
            illegal_groups: illegal,
        }
    }

    fn read_data(&self, at: usize) -> GcrData {
        let mut out = [0u8; DATA_BYTES];
        let illegal = self.read_block(at, &mut out);
        let payload = out[1..257].to_vec();
        let checksum = payload.iter().fold(0u8, |sum, &byte| sum ^ byte);
        GcrData {
            at_bit: at,
            bytes: payload,
            stored_checksum: out[257],
            computed_checksum: checksum,
            illegal_groups: illegal,
        }
    }

    /// Each header paired with the first data block starting after it,
    /// wrapping allowed for the same reason reads wrap.
    pub(crate) fn decode(&self) -> Vec<GcrSector> {
        let mut headers = Vec::new();
        let mut datas = Vec::new();
        for start in self.block_starts() {
            let high = gcr::decode_group(self.read_bits(start, 5));
            let low = gcr::decode_group(self.read_bits(start + 5, 5));
            match (high, low) {
                (Some(high), Some(low)) if (high << 4) | low == HEADER_MARK => {
                    headers.push(self.read_header(start));
                }
                (Some(high), Some(low)) if (high << 4) | low == DATA_MARK => {
                    datas.push(self.read_data(start));
                }
                _ => {}
            }
        }
        headers
            .into_iter()
            .map(|header| {
                let mut best: Option<&GcrData> = None;
                let mut best_distance = usize::MAX;
                for data in &datas {
                    let mut distance = data.at_bit as i64 - header.at_bit as i64;
                    if distance < 0 {
                        distance += self.bit_length as i64;
                    }
                    if (distance as usize) < best_distance {
                        best_distance = distance as usize;
                        best = Some(data);
                    }
                }
                GcrSector {
                    header,
                    data: best.cloned(),
                }
            })
            .collect()
    }
}

/// The CBM DOS recording layout: 35 tracks in four speed zones.
pub(crate) mod cbm_dos {
    pub(crate) const TRACKS: u8 = 35;
    const ZONES: [(u8, u8, u8); 4] = [(1, 17, 21), (18, 24, 19), (25, 30, 18), (31, 35, 17)];

    /// How many sectors the recording defines on `track`, or `None`
    /// off the 35-track grid.
    pub(crate) fn sectors_on(track: u8) -> Option<u8> {
        ZONES
            .iter()
            .find(|(first, last, _)| (*first..=*last).contains(&track))
            .map(|(_, _, sectors)| *sectors)
    }

    pub(crate) fn total_sectors() -> usize {
        (1..=TRACKS)
            .map(|track| usize::from(sectors_on(track).expect("every track is zoned")))
            .sum()
    }

    pub(crate) fn block_index(track: u8, sector: u8) -> Option<usize> {
        if sector >= sectors_on(track)? {
            return None;
        }
        let before: usize = (1..track)
            .map(|t| usize::from(sectors_on(t).expect("every track is zoned")))
            .sum();
        Some(before + usize::from(sector))
    }
}

/// A d64 being assembled: 683 blocks, first write wins, so a caller
/// puts its most trusted source in first.
pub(crate) struct D64Builder {
    blocks: Vec<u8>,
    present: Vec<bool>,
}

pub(crate) const SECTOR_SIZE: usize = 256;

impl D64Builder {
    pub(crate) fn new() -> Self {
        Self {
            blocks: vec![0; cbm_dos::total_sectors() * SECTOR_SIZE],
            present: vec![false; cbm_dos::total_sectors()],
        }
    }

    pub(crate) fn holds(&self, track: u8, sector: u8) -> bool {
        cbm_dos::block_index(track, sector).is_some_and(|index| self.present[index])
    }

    /// Places one block; the first write wins and a second is
    /// declined, not an error.
    pub(crate) fn put(&mut self, track: u8, sector: u8, data: &[u8]) -> bool {
        let Some(index) = cbm_dos::block_index(track, sector) else {
            return false;
        };
        if data.len() != SECTOR_SIZE || self.present[index] {
            return false;
        }
        self.blocks[index * SECTOR_SIZE..(index + 1) * SECTOR_SIZE].copy_from_slice(data);
        self.present[index] = true;
        true
    }

    pub(crate) fn filled(&self) -> usize {
        self.present.iter().filter(|&&held| held).count()
    }

    pub(crate) fn missing(&self) -> Vec<(u8, u8)> {
        let mut gaps = Vec::new();
        let mut index = 0usize;
        for track in 1..=cbm_dos::TRACKS {
            for sector in 0..cbm_dos::sectors_on(track).expect("every track is zoned") {
                if !self.present[index] {
                    gaps.push((track, sector));
                }
                index += 1;
            }
        }
        gaps
    }

    /// The d64's bytes: 683 blocks exactly when complete, and the
    /// per-block error map appended when anything is missing — the
    /// declared-loss account in the format's own spelling. Error code
    /// 1 is no error; 2 is header not found.
    pub(crate) fn into_bytes(self) -> Vec<u8> {
        if self.present.iter().all(|&held| held) {
            return self.blocks;
        }
        let mut out = self.blocks;
        out.reserve(self.present.len());
        for held in self.present {
            out.push(if held { 1 } else { 2 });
        }
        out
    }
}

/// The g64 grammar: `GCR-1541`, 84 half-track slots, offset and
/// speed-zone tables, then each present track padded to the widest.
pub(crate) const G64_HALF_TRACKS: usize = 84;
const G64_CONVENTIONAL_MAX_TRACK_BYTES: usize = 7928;

pub(crate) struct G64Track {
    pub(crate) index: u64,
    pub(crate) data: Vec<u8>,
    pub(crate) bit_length: usize,
    pub(crate) speed_zone: u8,
}

/// The 1541's four zone rates, as cells per revolution.
const ZONE_CELLS_PER_REVOLUTION: [f64; 4] = [50_000.0, 53_333.333, 57_142.857, 61_538.462];

pub(crate) fn nominal_cell(zone: usize) -> f64 {
    ANGULAR_DIVISIONS as f64 / ZONE_CELLS_PER_REVOLUTION[zone]
}

pub(crate) fn speed_zone_for(cell_divisions: f64) -> u8 {
    let mut best = 0u8;
    let mut best_distance = f64::MAX;
    for zone in 0..ZONE_CELLS_PER_REVOLUTION.len() {
        let distance = (cell_divisions - nominal_cell(zone)).abs();
        if distance < best_distance {
            best_distance = distance;
            best = zone as u8;
        }
    }
    best
}

pub(crate) fn g64_bytes(tracks: &[G64Track]) -> Result<Vec<u8>> {
    let mut widest = 0usize;
    let mut seen = std::collections::BTreeSet::new();
    for track in tracks {
        if track.index >= G64_HALF_TRACKS as u64 {
            return Err(Error::invalid_image(
                REMANENCE,
                format!("half-track {} sits outside the g64 grammar's 84 slots", track.index),
            ));
        }
        if !seen.insert(track.index) {
            return Err(Error::invalid_image(
                REMANENCE,
                format!("two tracks at half-track {}", track.index),
            ));
        }
        widest = widest.max(track.data.len());
    }
    let track_bytes = G64_CONVENTIONAL_MAX_TRACK_BYTES.max(widest);
    let stride = 2 + track_bytes;
    let header = 12usize;
    let tables = G64_HALF_TRACKS * 4 * 2;
    let mut file = vec![0u8; header + tables + tracks.len() * stride];
    file[..8].copy_from_slice(b"GCR-1541");
    file[8] = 0;
    file[9] = G64_HALF_TRACKS as u8;
    file[10] = track_bytes as u8;
    file[11] = (track_bytes >> 8) as u8;
    let mut at = header + tables;
    let mut ordered: Vec<&G64Track> = tracks.iter().collect();
    ordered.sort_by_key(|track| track.index);
    for track in ordered {
        let offsets = header + track.index as usize * 4;
        let zones = header + G64_HALF_TRACKS * 4 + track.index as usize * 4;
        file[offsets..offsets + 4].copy_from_slice(&(at as u32).to_le_bytes());
        file[zones..zones + 4].copy_from_slice(&u32::from(track.speed_zone).to_le_bytes());
        file[at] = track.data.len() as u8;
        file[at + 1] = (track.data.len() >> 8) as u8;
        file[at + 2..at + 2 + track.data.len()].copy_from_slice(&track.data);
        at += stride;
    }
    Ok(file)
}

/// What a rendition reports beside its artifact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct G64Report {
    pub tracks: u32,
    pub bits: u64,
    /// Half-tracks clocked at their zone nominal because their own
    /// measured figure was not a recording's.
    pub clocked_at_nominal: u32,
    pub skipped_points: u64,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct D64Report {
    pub blocks_read: u32,
    pub blocks_defined: u32,
    /// Sectors whose header or data failed its own checksum — recorded
    /// and left out, never repaired.
    pub failed_checksums: u32,
    pub missing: Vec<(u8, u8)>,
}

/// One orbit's coherent angles and the cell its recording implies.
fn clocked_orbit(image: &RemanenceImage, orbit: &Orbit) -> Result<(Vec<i64>, f64)> {
    let points = image.points(orbit)?;
    let angles: Vec<i64> = points
        .iter()
        .filter(|point| point.magnetization().is_coherent())
        .map(|point| point.angle() as i64)
        .collect();
    let cell = cell_of(&angles, ANGULAR_DIVISIONS as i64);
    Ok((angles, cell))
}

impl RemanenceDisk {
    /// The g64 rendition: every on-grid orbit clocked at its measured
    /// cell — or the zone nominal where the measured figure is not a
    /// recording's, since clocking noise at its own "cell" would run
    /// several revolutions long — packed under the `GCR-1541` grammar.
    pub fn write_g64(&self, path: impl AsRef<Path>) -> Result<G64Report> {
        let (tracks, report) = self.g64_tracks()?;
        let bytes = g64_bytes(&tracks)?;
        write_whole(path.as_ref(), &bytes)?;
        Ok(report)
    }

    fn g64_tracks(&self) -> Result<(Vec<G64Track>, G64Report)> {
        let image = self.image();
        let mut tracks = Vec::new();
        let mut notes = Vec::new();
        let mut bits = 0u64;
        let mut skipped_points = 0u64;
        let mut clocked_at_nominal = 0u32;
        for orbit in image.orbits() {
            let Some(step) = grid_step_of(orbit.key().radius_microns()) else {
                notes.push(format!(
                    "orbit at {} microns sits off the 96 tpi grid; the g64 grammar \
                     cannot place it",
                    orbit.key().radius_microns()
                ));
                continue;
            };
            if step >= G64_HALF_TRACKS as u64 {
                continue;
            }
            let (angles, measured) = clocked_orbit(image, orbit)?;
            skipped_points += orbit.points() - angles.len() as u64;
            if angles.len() < 2 {
                notes.push(format!(
                    "half-track {step} holds {} transitions; nothing to clock",
                    angles.len()
                ));
                continue;
            }
            let zone = speed_zone_for(measured);
            let mut clock_cell = measured;
            if (measured - nominal_cell(zone.into())).abs() > nominal_cell(zone.into()) * 0.1 {
                clock_cell = nominal_cell(zone.into());
                clocked_at_nominal += 1;
                notes.push(format!(
                    "half-track {step} clocked at zone {zone} nominal; its measured \
                     cell {} is not a recording's",
                    measured.round()
                ));
            }
            let stream = clock(&angles, clock_cell);
            bits += stream.len();
            let bit_length = stream.len() as usize;
            tracks.push(G64Track {
                index: step,
                data: stream.into_bytes(),
                bit_length,
                speed_zone: zone,
            });
        }
        let report = G64Report {
            tracks: tracks.len() as u32,
            bits,
            clocked_at_nominal,
            skipped_points,
            notes,
        };
        Ok((tracks, report))
    }

    /// The d64 rendition: the recording's own sectors — every orbit
    /// clocked and decoded, a block admitted where both its header and
    /// its data pass their own checksums, addressed by the header's
    /// own track and sector. First write wins, whole tracks before
    /// half-tracks. An incomplete disk carries the error map.
    pub fn write_d64(&self, path: impl AsRef<Path>) -> Result<D64Report> {
        let (builder, report) = self.d64_blocks()?;
        write_whole(path.as_ref(), &builder.into_bytes())?;
        Ok(report)
    }

    fn d64_blocks(&self) -> Result<(D64Builder, D64Report)> {
        let image = self.image();
        let mut builder = D64Builder::new();
        let mut failed = 0u32;
        // Whole tracks first — the recording's own grid — then the
        // half-tracks between them, so a fat track's shoulder never
        // outbids its center.
        let mut orbits: Vec<(&Orbit, u64)> = image
            .orbits()
            .filter_map(|orbit| {
                grid_step_of(orbit.key().radius_microns()).map(|step| (orbit, step))
            })
            .collect();
        orbits.sort_by_key(|(_, step)| (step % 2, *step));
        for (orbit, _) in orbits {
            let (angles, cell) = clocked_orbit(image, orbit)?;
            if angles.len() < 2 || cell <= 0.0 {
                continue;
            }
            let stream = clock(&angles, cell);
            let bit_length = stream.len() as usize;
            if bit_length == 0 {
                continue;
            }
            let track = GcrTrack::new(stream.into_bytes(), bit_length);
            for sector in track.decode() {
                let Some(data) = sector.data else {
                    failed += 1;
                    continue;
                };
                if !sector.header.checksum_ok() || !data.checksum_ok() {
                    failed += 1;
                    continue;
                }
                builder.put(sector.header.track, sector.header.sector, &data.bytes);
            }
        }
        let report = D64Report {
            blocks_read: builder.filled() as u32,
            blocks_defined: cbm_dos::total_sectors() as u32,
            failed_checksums: failed,
            missing: builder.missing(),
        };
        Ok((builder, report))
    }

    /// The p64 rendition: one multiply from angle to cycle, coherent
    /// points only, an orbit with no pulses skipped rather than
    /// written empty — an absent half-track claims never-written,
    /// which is what `Unaligned` means, where an empty chunk would
    /// claim formatted-then-erased — served through the delivered P64
    /// encode path.
    pub fn write_p64(&self, path: impl AsRef<Path>) -> Result<crate::P64Report> {
        let medium = self.served_medium()?;
        crate::p64::write_new_artifact(&medium, path.as_ref())
    }

    pub fn describe_p64(&self) -> Result<crate::P64Report> {
        let medium = self.served_medium()?;
        crate::p64::describe(&medium, None)
    }

    /// The remanence artifact itself: the image encoded whole under
    /// its own grammar (F64).
    pub fn write_remanence(&self, path: impl AsRef<Path>) -> Result<()> {
        let bytes = crate::remanence_format::to_bytes(self.image())?;
        write_whole(path.as_ref(), &bytes)
    }

    /// The served projection this rendition path needs: the image's
    /// coherent points at the 1541's reference frame, full strength —
    /// the remanence model deliberately carries no per-pulse strength,
    /// uncertainty living in the report instead.
    fn served_medium(&self) -> Result<crate::flux_medium::FluxMedium> {
        use crate::flux_capture::TimeBase;
        use crate::flux_medium::{
            Derivation, LocationKey, MediumBuilder, OriginRule, OriginStatement, Pulse,
            RotationalFrame, Strength,
        };
        let image = self.image();
        let frame = RotationalFrame::new(
            "c1541",
            TimeBase::new("c1541", 16_000_000, 1)?,
            CYCLES_PER_ROTATION,
            OriginStatement::new(
                OriginRule::Index,
                Provenance::new(REMANENCE)
                    .note("the image's angular origin is the capture's index datum"),
            ),
        )?;
        let policy = Provenance::new(REMANENCE)
            .note("served projection of a remanence image: angle times 3,200,000 over 2^28")
            .note("coherent points only; full strength — uncertainty rides the report");
        let sink = crate::flux_capture::SessionBacking::create()?;
        let mut builder = MediumBuilder::new(
            "c1541",
            crate::drive_profile::C1541.media,
            frame,
            Derivation::SelectedAndProjected,
            policy,
            sink,
        )?;

        // Ascending half-track keys are descending radii: outermost
        // first.
        let mut orbits: Vec<(&Orbit, u64)> = image
            .orbits()
            .filter_map(|orbit| {
                grid_step_of(orbit.key().radius_microns()).map(|step| (orbit, step))
            })
            .filter(|(_, step)| *step + 2 <= 85)
            .collect();
        orbits.sort_by_key(|(_, step)| *step);
        let mut added = false;
        for (orbit, step) in orbits {
            let points = image.points(orbit)?;
            let mut pulses: Vec<Pulse> = Vec::new();
            let mut previous: i64 = -1;
            for point in &points {
                if !point.magnetization().is_coherent() {
                    continue;
                }
                let scaled = (u128::from(point.angle()) * u128::from(CYCLES_PER_ROTATION)
                    + u128::from(ANGULAR_DIVISIONS / 2))
                    / u128::from(ANGULAR_DIVISIONS);
                let mut position = (scaled as u64).min(CYCLES_PER_ROTATION - 1) as i64;
                // Rounding collisions nudge to the next cycle; the
                // delivered container refuses non-advancing pulses.
                if position <= previous {
                    position = previous + 1;
                    if position as u64 >= CYCLES_PER_ROTATION {
                        return Err(Error::invalid_image(
                            REMANENCE,
                            format!(
                                "half-track {} overruns the revolution on a rounding \
                                 collision",
                                step + 2
                            ),
                        ));
                    }
                }
                pulses.push(Pulse::new(position as u64, Strength::certain(2)));
                previous = position;
            }
            if pulses.is_empty() {
                continue;
            }
            let key = LocationKey::fraction("c1541", step + 2, 2, Some(0))?;
            builder.add_location(key, &pulses, &[], Provenance::new(REMANENCE))?;
            added = true;
        }
        if !added {
            return Err(Error::invalid_image(
                REMANENCE,
                "the image holds no on-grid orbit with any coherent point to serve",
            ));
        }
        let (mut medium, sink, total) = builder.seal()?;
        medium.attach_backing(
            Box::new(sink.into_source()),
            total,
            crate::cache::DEFAULT_CACHE_BYTES,
        );
        Ok(medium)
    }
}

fn write_whole(path: &Path, bytes: &[u8]) -> Result<()> {
    if path.exists() {
        return Err(Error::invalid_image(
            REMANENCE,
            format!("destination {path:?} already exists; a rendition never overwrites"),
        ));
    }
    std::fs::write(path, bytes)
        .map_err(|error| Error::io(format!("failed to write the rendition: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_grid_places_and_refuses_radii() {
        assert_eq!(grid_step_of(57_150), Some(0));
        assert_eq!(grid_step_of(56_885), Some(1));
        assert_eq!(grid_step_of(39_688), Some(66));
        // Between steps: 57,150 − 132 sits half a step off.
        assert_eq!(grid_step_of(57_018), None);
    }

    #[test]
    fn the_group_code_round_trips_and_refuses() {
        for nibble in 0u8..16 {
            assert_eq!(gcr::decode_group(gcr::encode_nibble(nibble)), Some(nibble));
        }
        assert_eq!(gcr::decode_group(0b00000), None);
        assert_eq!(gcr::decode_group(0b11111), None);
    }

    /// A synthetic sector: sync, header, gap, sync, data — encoded
    /// through the group code, clocked back out of the decoder.
    fn encoded_track(track: u8, sector: u8, payload: &[u8; 256]) -> (Vec<u8>, usize) {
        let mut stream = Bitstream::new();
        let mut write_byte = |stream: &mut Bitstream, byte: u8| {
            for group in [gcr::encode_nibble(byte >> 4), gcr::encode_nibble(byte)] {
                for bit in (0..5).rev() {
                    if group & (1 << bit) != 0 {
                        stream.append_one();
                    } else {
                        stream.append_zeros(1);
                    }
                }
            }
        };
        let sync = |stream: &mut Bitstream| {
            for _ in 0..12 {
                stream.append_one();
            }
        };

        let id = (0x50u8, 0x43u8);
        sync(&mut stream);
        let header_checksum = sector ^ track ^ id.1 ^ id.0;
        for byte in [HEADER_MARK, header_checksum, sector, track, id.1, id.0, 0x0f, 0x0f] {
            write_byte(&mut stream, byte);
        }
        stream.append_zeros(9 * 8);
        sync(&mut stream);
        write_byte(&mut stream, DATA_MARK);
        let mut checksum = 0u8;
        for &byte in payload {
            write_byte(&mut stream, byte);
            checksum ^= byte;
        }
        write_byte(&mut stream, checksum);
        write_byte(&mut stream, 0);
        write_byte(&mut stream, 0);
        stream.append_zeros(64);

        let length = stream.len() as usize;
        (stream.into_bytes(), length)
    }

    #[test]
    fn the_decoder_reads_its_own_recording() {
        let payload = {
            let mut bytes = [0u8; 256];
            for (i, byte) in bytes.iter_mut().enumerate() {
                *byte = (i % 251) as u8;
            }
            bytes
        };
        let (bits, length) = encoded_track(17, 3, &payload);
        let track = GcrTrack::new(bits, length);
        let sectors = track.decode();
        assert_eq!(sectors.len(), 1);
        let sector = &sectors[0];
        assert_eq!(sector.header.track, 17);
        assert_eq!(sector.header.sector, 3);
        assert!(sector.header.checksum_ok());
        let data = sector.data.as_ref().expect("the data block pairs");
        assert!(data.checksum_ok());
        assert_eq!(data.bytes, payload);
    }

    #[test]
    fn the_d64_grid_counts_the_zones() {
        assert_eq!(cbm_dos::sectors_on(1), Some(21));
        assert_eq!(cbm_dos::sectors_on(17), Some(21));
        assert_eq!(cbm_dos::sectors_on(18), Some(19));
        assert_eq!(cbm_dos::sectors_on(31), Some(17));
        assert_eq!(cbm_dos::sectors_on(36), None);
        assert_eq!(cbm_dos::total_sectors(), 683);

        let mut builder = D64Builder::new();
        assert!(builder.put(18, 0, &[0xaa; 256]));
        assert!(!builder.put(18, 0, &[0xbb; 256]), "first write wins");
        assert!(!builder.put(36, 0, &[0xcc; 256]), "off the grid");
        assert_eq!(builder.filled(), 1);
        let bytes = builder.into_bytes();
        assert_eq!(bytes.len(), 683 * 256 + 683, "incomplete carries the error map");
        assert_eq!(bytes[683 * 256 + cbm_dos::block_index(18, 0).unwrap()], 1);
        assert_eq!(bytes[683 * 256], 2, "t01/s00 was never read");
    }

    /// The whole rendition path over the repository's capture
    /// fixture, checked against the artifacts the research lineage
    /// rendered from the same disk. Checksummed sectors are the
    /// strongest comparison there is: any block both renditions hold
    /// must be byte-identical, whatever floating-point daylight
    /// separates the two reconstructions' angles.
    #[test]
    fn the_pinball_renditions_match_the_lineage() {
        let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let capture_path =
            fixtures.join("Bill Budge Pinball Construction Set [Commodore 64] (1of2).7z");
        let golden_d64_path = fixtures.join("pinball-construction-set-c64.d64");
        let golden_g64_path = fixtures.join("pinball-construction-set-c64.front.g64");
        if !capture_path.exists() {
            panic!(
                "missing fixture {capture_path:?}: run `uv run --group testing-prep \
                 testing-prep/prep_fixtures.py`"
            );
        }
        let Ok(golden_d64) = std::fs::read(&golden_d64_path) else {
            eprintln!("skipping: golden d64 not present at {golden_d64_path:?}");
            return;
        };

        let _gate = crate::remanence_reconstruction::CAPTURE_FIXTURE_GATE
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let set = crate::kryoflux::CaptureSet::open(&capture_path).expect("the capture opens");
        let plan = crate::remanence_reconstruction::plan(
            set.capture(),
            &crate::remanence_reconstruction::ReconstructionPolicy {
                side: 0,
                recordings: crate::remanence_reconstruction::RecordingSelection::Measured,
            },
        )
        .expect("the reduction plans");
        let disk = plan
            .execute(crate::cache::DEFAULT_CACHE_BYTES)
            .expect("the plan executes");

        // ---- d64: block-for-block against the lineage ----
        let (builder, report) = disk.d64_blocks().expect("the d64 assembles");
        assert!(
            report.blocks_read >= 600,
            "the reference disk reads most of its 683 blocks: {}",
            report.blocks_read
        );
        let golden_blocks = &golden_d64[..683 * 256];
        let golden_present: Vec<bool> = if golden_d64.len() > 683 * 256 {
            golden_d64[683 * 256..].iter().map(|&code| code == 1).collect()
        } else {
            vec![true; 683]
        };
        let mine = builder.into_bytes();
        let mut shared = 0usize;
        let mut index = 0usize;
        for track in 1..=cbm_dos::TRACKS {
            for sector in 0..cbm_dos::sectors_on(track).expect("every track is zoned") {
                let held_here = !report.missing.contains(&(track, sector));
                if held_here && golden_present[index] {
                    let at = index * SECTOR_SIZE;
                    assert_eq!(
                        &mine[at..at + SECTOR_SIZE],
                        &golden_blocks[at..at + SECTOR_SIZE],
                        "block t{track:02}/s{sector:02} differs from the lineage"
                    );
                    shared += 1;
                }
                index += 1;
            }
        }
        assert!(
            shared >= 600,
            "the two renditions share most of the disk: {shared} blocks"
        );

        // ---- g64: the same half-tracks at the same zones ----
        if let Ok(golden_g64) = std::fs::read(&golden_g64_path) {
            let (tracks, _) = disk.g64_tracks().expect("the g64 assembles");
            assert_eq!(&golden_g64[..8], b"GCR-1541");
            for track in &tracks {
                let offset_at = 12 + track.index as usize * 4;
                let golden_offset = u32::from_le_bytes(
                    golden_g64[offset_at..offset_at + 4].try_into().unwrap(),
                );
                assert!(
                    golden_offset != 0,
                    "half-track {} present here but absent in the lineage",
                    track.index
                );
                let zone_at = 12 + G64_HALF_TRACKS * 4 + track.index as usize * 4;
                let golden_zone =
                    u32::from_le_bytes(golden_g64[zone_at..zone_at + 4].try_into().unwrap());
                assert_eq!(
                    u32::from(track.speed_zone),
                    golden_zone,
                    "half-track {} zones differently from the lineage",
                    track.index
                );
            }
        } else {
            eprintln!("skipping g64 comparison: golden not present");
        }

        // ---- p64: through the delivered decoder, whole ----
        let scratch = std::env::temp_dir().join(format!(
            "remanence-rendition-test-{}.p64",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&scratch);
        let written = disk.write_p64(&scratch).expect("the p64 writes");
        let reopened =
            crate::P64Image::open(&scratch).expect("the delivered adapter reads it back");
        let restored: u64 = reopened
            .inspect()
            .half_tracks
            .iter()
            .map(|half_track| half_track.pulses)
            .sum();
        let stated: u64 = written
            .half_tracks
            .iter()
            .map(|half_track| half_track.pulses)
            .sum();
        assert_eq!(restored, stated, "the round trip loses nothing");
        assert!(
            restored > 1_000_000,
            "a whole side carries over a million pulses: {restored}"
        );
        drop(reopened);
        let _ = std::fs::remove_file(&scratch);
    }

    #[test]
    fn the_g64_grammar_lays_out_tables_and_padding() {
        let tracks = vec![G64Track {
            index: 0,
            data: vec![0x55; 100],
            bit_length: 800,
            speed_zone: 3,
        }];
        let bytes = g64_bytes(&tracks).expect("one track lays out");
        assert_eq!(&bytes[..8], b"GCR-1541");
        assert_eq!(bytes[9], 84);
        let track_bytes = usize::from(bytes[10]) | usize::from(bytes[11]) << 8;
        assert_eq!(track_bytes, 7928, "the conventional width holds small tracks");
        let offset = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
        assert_eq!(offset, 12 + 84 * 8);
        assert_eq!(
            u32::from_le_bytes(bytes[12 + 84 * 4..16 + 84 * 4].try_into().unwrap()),
            3
        );
        assert_eq!(bytes[offset], 100);
        assert_eq!(bytes[offset + 2], 0x55);
        assert_eq!(bytes.len(), 12 + 84 * 8 + 2 + 7928);
    }
}
