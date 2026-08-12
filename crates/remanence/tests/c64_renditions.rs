// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The C64 renditions, from outside the crate.
//!
//! A remanence image renders to d64, g64 and p64 — P29 acting where
//! only the destination varies, each stating its loss. All three are
//! reached through the public surface alone, which is the point: the
//! group code, the sector reading and the clocking beneath them are
//! the library's own analysis machinery and are not reachable here.
//!
//! The disk under test is built by hand rather than fetched, so the
//! whole path is exercised on every host: this file lays a real
//! `GCR-1541` recording of track one — twenty-one sectors, sync,
//! headers, data blocks and checksums, encoded through its own copy of
//! the group code — clocks it into transition angles at the zone's
//! rate, and wraps those in a `.remanence` artifact. The group code
//! here is written against the format rather than shared with the
//! library, so a rendition matching it is two implementations
//! agreeing. The artifacts the research lineage rendered from a real
//! capture are the scale check beside it, and skip when absent.

use std::path::PathBuf;

use remanence::RemanenceImage;

mod common;

// ------------------------------------------------------------ the disk

/// The 1541's 4-to-5 group code, written from the format rather than
/// shared with the library's own table.
const GCR: [u8; 16] = [
    0b01010, 0b01011, 0b10010, 0b10011, 0b01110, 0b01111, 0b10110, 0b10111, 0b01001, 0b11001,
    0b11010, 0b11011, 0b01101, 0b11101, 0b11110, 0b10101,
];

const HEADER_MARK: u8 = 0x08;
const DATA_MARK: u8 = 0x07;
const DISK_ID: (u8, u8) = (0x50, 0x43);

/// Tracks 1 through 17 are the outermost zone: 21 sectors, and the
/// fastest of the drive's four rates.
const SECTORS_ON_TRACK_ONE: u8 = 21;
const ZONE: u8 = 3;
const CELLS_PER_REVOLUTION: u64 = 61_538;
const ANGULAR_DIVISIONS: u64 = 1 << 28;

/// The payload of one sector, distinct per sector so a block landing in
/// the wrong slot of the grid is caught rather than matching by luck.
fn payload_of(sector: u8) -> [u8; 256] {
    let mut bytes = [0u8; 256];
    for (i, byte) in bytes.iter_mut().enumerate() {
        *byte = ((i * 7 + usize::from(sector) * 29) % 251) as u8;
    }
    bytes
}

/// Channel bits in read order, most significant bit of each byte first.
struct Bits(Vec<bool>);

impl Bits {
    fn one(&mut self) {
        self.0.push(true);
    }

    fn zeros(&mut self, count: usize) {
        self.0.extend(std::iter::repeat_n(false, count));
    }

    fn sync(&mut self) {
        for _ in 0..40 {
            self.one();
        }
    }

    fn byte(&mut self, byte: u8) {
        for group in [GCR[usize::from(byte >> 4)], GCR[usize::from(byte & 0x0f)]] {
            for bit in (0..5).rev() {
                if group & (1 << bit) != 0 {
                    self.one();
                } else {
                    self.zeros(1);
                }
            }
        }
    }

    /// A gap, in the byte the drive fills one with. It is written
    /// through the group code like everything else, which is what keeps
    /// a gap's transition density that of the recording around it.
    fn gap(&mut self, bytes: usize) {
        for _ in 0..bytes {
            self.byte(0x55);
        }
    }
}

/// Track one as the drive would have written it: twenty-one sectors,
/// each a sync, a header, a gap, a sync and a data block, padded out to
/// the zone's own count of cells so the revolution closes.
fn track_one_bits() -> Vec<bool> {
    let mut bits = Bits(Vec::new());
    for sector in 0..SECTORS_ON_TRACK_ONE {
        let checksum = sector ^ 1 ^ DISK_ID.1 ^ DISK_ID.0;
        bits.sync();
        for byte in [
            HEADER_MARK,
            checksum,
            sector,
            1,
            DISK_ID.1,
            DISK_ID.0,
            0x0f,
            0x0f,
        ] {
            bits.byte(byte);
        }
        bits.gap(9);
        bits.sync();
        bits.byte(DATA_MARK);
        let payload = payload_of(sector);
        let mut checksum = 0u8;
        for &byte in &payload {
            bits.byte(byte);
            checksum ^= byte;
        }
        bits.byte(checksum);
        bits.byte(0);
        bits.byte(0);
        bits.gap(8);
    }
    let laid = bits.0.len();
    assert!(
        laid < CELLS_PER_REVOLUTION as usize,
        "the recording fits one revolution: {laid} cells"
    );
    bits.zeros(CELLS_PER_REVOLUTION as usize - laid);
    bits.0
}

/// Where each recorded bit sits on the circle, at the zone's rate. The
/// image holds angles and no clock: the cell length is a property of
/// this recording, recovered from the angles rather than stated beside
/// them.
fn track_one_angles() -> Vec<u64> {
    track_one_bits()
        .into_iter()
        .enumerate()
        .filter(|(_, recorded)| *recorded)
        .map(|(cell, _)| {
            (cell as u128 * u128::from(ANGULAR_DIVISIONS) / u128::from(CELLS_PER_REVOLUTION)) as u64
        })
        .collect()
}

// -------------------------------------------------------- the artifact

fn varint(out: &mut Vec<u8>, mut value: u64) {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

/// The payload grammar: form factor, holes, then the surfaces, each
/// naming itself, and the orbits as varint angle deltas under the
/// two-bit tag. Only the first point states its sense and the write
/// geometry; the rest alternate, which is what the model derives.
fn payload(radius_microns: u64, angles: &[u64]) -> Vec<u8> {
    let mut out = vec![0x01]; // 5.25-inch
    varint(&mut out, 1); // one hole
    varint(&mut out, 6); // centre 3/8 of a turn, zigzagged
    varint(&mut out, 8);
    varint(&mut out, 2); // extent 1/50, zigzagged
    varint(&mut out, 50);
    varint(&mut out, 1); // one surface
    varint(&mut out, 0); // surface 0
    varint(&mut out, 1); // one orbit
    varint(&mut out, radius_microns);
    varint(&mut out, angles.len() as u64);
    let mut previous = 0u64;
    for (index, &angle) in angles.iter().enumerate() {
        if index == 0 {
            varint(&mut out, (angle << 2) | 0b11);
            out.push(0); // positive
            varint(&mut out, 330); // plateau, the standard's own plateau
            varint(&mut out, 432); // guard
        } else {
            varint(&mut out, (angle - previous) << 2);
        }
        previous = angle;
    }
    out
}

fn adler32(bytes: &[u8]) -> u32 {
    let (mut low, mut high) = (1u32, 0u32);
    for byte in bytes {
        low = (low + u32::from(*byte)) % 65521;
        high = (high + low) % 65521;
    }
    (high << 16) | low
}

/// The payload as one zlib stream of stored DEFLATE blocks — the
/// simplest valid encoding, and one no compressor is needed to emit.
fn zlib_stored(payload: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    for (index, block) in payload.chunks(0xffff).enumerate() {
        let last = (index + 1) * 0xffff >= payload.len();
        out.push(u8::from(last));
        out.extend_from_slice(&(block.len() as u16).to_le_bytes());
        out.extend_from_slice(&(!(block.len() as u16)).to_le_bytes());
        out.extend_from_slice(block);
    }
    out.extend_from_slice(&adler32(payload).to_be_bytes());
    out
}

fn artifact(payload: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::from(*b"REMANENCE_PHYSICAL_DISK");
    bytes.push(0x1a);
    bytes.push(1);
    bytes.extend_from_slice(&zlib_stored(payload));
    bytes
}

fn scratch(tag: &str, extension: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "remanence-rendition-{tag}-{}.{extension}",
        std::process::id()
    ))
}

fn placed(tag: &str, bytes: &[u8]) -> PathBuf {
    let path = scratch(tag, "remanence");
    let _ = std::fs::remove_file(&path);
    std::fs::write(&path, bytes).expect("the scratch artifact is written");
    path
}

/// One image holding track one alone, at the radius a 96 tpi drive's
/// first step position sits at.
fn track_one_image(tag: &str) -> (RemanenceImage, PathBuf) {
    let path = placed(tag, &artifact(&payload(57_150, &track_one_angles())));
    let image = RemanenceImage::open(&path).expect("the hand-built disk opens");
    (image, path)
}

fn loss(report: &[remanence::DeclaredLoss], code: &str) -> Option<u64> {
    report
        .iter()
        .find(|entry| entry.code == code)
        .map(|entry| entry.count)
}

// ------------------------------------------------------------ the d64

#[test]
fn the_d64_reads_the_recording_it_was_given() {
    let (image, source) = track_one_image("d64-source");

    let described = image.describe_d64().expect("the d64 is computed");
    assert_eq!(described.path, None, "describing writes nothing");
    assert_eq!(described.blocks_defined, 683);
    assert_eq!(
        described.blocks_read, 21,
        "track one's twenty-one sectors are read and nothing else is"
    );
    assert_eq!(described.failed_checksums, 0);
    assert_eq!(described.missing.len(), 683 - 21);
    assert!(
        described.missing.iter().all(|block| block.track != 1),
        "nothing on track one is missing"
    );

    // The account states what the destination did not carry, and the
    // error map is that account made flesh.
    assert_eq!(loss(&described.declared_loss, "block-not-read"), Some(662));
    assert_eq!(
        loss(&described.declared_loss, "recording-structure"),
        Some(1)
    );
    assert_eq!(described.artifact_bytes, 683 * 256 + 683);

    let destination = scratch("d64-written", "d64");
    let _ = std::fs::remove_file(&destination);
    let written = image.write_d64(&destination).expect("the d64 writes");
    assert_eq!(
        written.path.as_deref(),
        Some(destination.to_string_lossy().as_ref())
    );
    assert_eq!(written.blocks_read, described.blocks_read);
    assert_eq!(written.declared_loss, described.declared_loss);
    assert_eq!(written.missing, described.missing);

    // Block for block against what was recorded: the group code the
    // library read with and the one this file wrote with agree.
    let bytes = std::fs::read(&destination).expect("the artifact is there");
    assert_eq!(bytes.len() as u64, written.artifact_bytes);
    for sector in 0..SECTORS_ON_TRACK_ONE {
        let at = usize::from(sector) * 256;
        assert_eq!(
            &bytes[at..at + 256],
            &payload_of(sector)[..],
            "block t01/s{sector:02} is not what was recorded"
        );
        assert_eq!(bytes[683 * 256 + usize::from(sector)], 1, "no error here");
    }
    assert_eq!(
        bytes[683 * 256 + 21],
        2,
        "the first block the recording never held is header-not-found"
    );

    drop(image);
    for path in [source, destination] {
        let _ = std::fs::remove_file(&path);
    }
}

// ------------------------------------------------------------ the g64

#[test]
fn the_g64_packs_the_orbit_at_its_measured_zone() {
    let (image, source) = track_one_image("g64-source");

    let described = image.describe_g64().expect("the g64 is computed");
    assert_eq!(described.path, None, "describing writes nothing");
    assert_eq!(described.half_tracks.len(), 1);
    let half_track = described.half_tracks[0];
    assert_eq!(half_track.index, 0, "the outermost orbit is slot zero");
    assert_eq!(half_track.speed_zone, ZONE);
    assert!(
        !half_track.clocked_at_nominal,
        "a real recording clocks at its own measured cell"
    );
    assert_eq!(
        half_track.bits, CELLS_PER_REVOLUTION,
        "the clocked orbit is as long as the revolution it was written over"
    );

    // A g64 records a clocked bit and nothing about how wide the
    // crystal wrote it.
    assert_eq!(loss(&described.declared_loss, "write-geometry"), Some(1));
    assert_eq!(loss(&described.declared_loss, "measured-radius"), Some(1));

    let destination = scratch("g64-written", "g64");
    let _ = std::fs::remove_file(&destination);
    let written = image.write_g64(&destination).expect("the g64 writes");
    assert_eq!(written.half_tracks, described.half_tracks);
    assert_eq!(written.declared_loss, described.declared_loss);

    let bytes = std::fs::read(&destination).expect("the artifact is there");
    assert_eq!(bytes.len() as u64, written.artifact_bytes);
    assert_eq!(&bytes[..8], b"GCR-1541", "the grammar names itself");
    assert_eq!(bytes[9], 84, "eighty-four half-track slots");
    let offset = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    assert_eq!(
        offset,
        12 + 84 * 8,
        "slot zero is the first track laid down"
    );
    assert_eq!(
        u32::from_le_bytes(bytes[12 + 84 * 4..16 + 84 * 4].try_into().unwrap()),
        u32::from(ZONE),
        "the slot's speed zone table entry"
    );
    let stored = usize::from(bytes[offset]) | usize::from(bytes[offset + 1]) << 8;
    assert_eq!(
        stored as u64,
        CELLS_PER_REVOLUTION.div_ceil(8),
        "the track's own byte length precedes it"
    );

    // Every other slot is absent rather than empty: this disk holds one
    // orbit and claims nothing about the rest.
    for slot in 1..84usize {
        let at = 12 + slot * 4;
        assert_eq!(
            u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()),
            0,
            "slot {slot} was never written and says so"
        );
    }

    drop(image);
    for path in [source, destination] {
        let _ = std::fs::remove_file(&path);
    }
}

// ------------------------------------------------------------ the p64

#[test]
fn the_p64_projects_every_coherent_point_and_reads_back() {
    let (image, source) = track_one_image("p64-source");
    let points: u64 = image
        .inspect()
        .orbits
        .iter()
        .map(|orbit| orbit.points)
        .sum();

    let described = image.describe_p64().expect("the p64 is computed");
    assert_eq!(described.half_tracks.len(), 1);
    assert_eq!(
        described.half_tracks[0].pulses, points,
        "every coherent point crosses; this disk has nothing else"
    );
    assert_eq!(
        (
            described.half_tracks[0].half_track_numerator,
            described.half_tracks[0].half_track_denominator,
        ),
        (1, 1),
        "the outermost orbit is the whole track one"
    );
    // The projection's own account rides the container's.
    assert_eq!(loss(&described.declared_loss, "write-geometry"), Some(1));
    assert_eq!(loss(&described.declared_loss, "measured-radius"), Some(1));

    let destination = scratch("p64-written", "p64");
    let _ = std::fs::remove_file(&destination);
    let written = image.write_p64(&destination).expect("the p64 writes");
    assert_eq!(written.half_tracks, described.half_tracks);
    assert_eq!(written.declared_loss, described.declared_loss);

    // Through the delivered load, back out again: the artifact is a
    // medium now (F59), and the served form loads straight in. A clean
    // synthetic recording detects one bit per pulse, so the bitstream's
    // own count is the round trip's witness.
    let mut session = remanence::Session::new();
    let disk = session
        .load_media(
            std::fs::File::open(&destination).expect("the artifact opens"),
            remanence::Format::P64,
        )
        .expect("the delivered load reads it back");
    let restored: u64 = disk
        .bitstream()
        .expect("the channel clocks it")
        .inspect()
        .locations
        .iter()
        .map(|location| location.one_bits)
        .sum();
    assert_eq!(restored, points, "the round trip loses no pulse");

    drop(image);
    drop(session);
    for path in [source, destination] {
        let _ = std::fs::remove_file(&path);
    }
}

// ------------------------------------------------------- the refusals

#[test]
fn an_occupied_destination_is_refused_by_every_rendition() {
    let (image, source) = track_one_image("occupied-source");
    let occupied = scratch("occupied-destination", "d64");
    let _ = std::fs::remove_file(&occupied);
    std::fs::write(&occupied, b"content this library did not write").expect("the occupant lands");

    for refusal in [
        image.write_d64(&occupied).err(),
        image.write_g64(&occupied).err(),
        image.write_p64(&occupied).err(),
    ] {
        let refusal = refusal.expect("an occupied destination is not written through");
        assert!(
            refusal.to_string().contains("already there"),
            "the refusal names what is in the way: {refusal}"
        );
    }
    assert_eq!(
        std::fs::read(&occupied).expect("the occupant is still there"),
        b"content this library did not write",
        "the refusals left the occupant untouched"
    );

    drop(image);
    for path in [source, occupied] {
        let _ = std::fs::remove_file(&path);
    }
}

#[test]
fn a_disk_the_grid_cannot_place_refuses_by_name() {
    // Half a step in from the first: a radius the 96 tpi grid has no
    // slot for, refused rather than pulled onto a neighbour.
    let path = placed("off-grid", &artifact(&payload(57_018, &track_one_angles())));
    let image = RemanenceImage::open(&path).expect("the off-grid disk opens");

    let refusal = image
        .describe_g64()
        .expect_err("a disk with no placeable orbit renders to nothing");
    assert!(
        refusal.to_string().contains("no orbit"),
        "the refusal says what was missing: {refusal}"
    );
    assert!(
        image.describe_p64().is_err(),
        "the p64 has nowhere to put it"
    );

    // The d64 is addressed by what the recording says of itself rather
    // than by where the orbit sits, so an unplaceable orbit is an empty
    // disk with a full error map rather than a refusal.
    let d64 = image.describe_d64().expect("the d64 is computed");
    assert_eq!(d64.blocks_read, 0);
    assert_eq!(loss(&d64.declared_loss, "orbit-off-grid"), Some(1));

    drop(image);
    let _ = std::fs::remove_file(&path);
}

// ------------------------------------------------------- at real scale
