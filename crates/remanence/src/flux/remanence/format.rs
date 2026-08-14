// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The `.remanence` artifact (F64): the library's own flux format,
//! claimed in both directions.
//!
//! The grammar: the ASCII magic, a 0x1A binary sentinel — a reader
//! meeting the text grammar this format once had stops there — and a
//! one-byte layout version gated first (P8), then one zlib-framed
//! DEFLATE stream holding the whole image. Inside: the form-factor
//! code; the holes as exact rationals; the surfaces, each naming
//! itself; the orbits outermost-first; and the points as varint angle
//! deltas under a two-bit tag electing a magnetization byte and the
//! two width varints. The magnetization byte is elided wherever the
//! model's own alternation invariant derives it — it survives at an
//! orbit's first coherent point, at one reopening after a span with no
//! sense, and at a width-stating splice repeating its predecessor's
//! polarity. Counts, never terminators.
//!
//! Structural rules — ascending angles, alternation, the first
//! coherent point stating widths — are enforced by the model's
//! constructors, not re-checked here, so a file and a constructed
//! image are held to one standard.
//!
//! The writer's output is deterministic — same image, same bytes —
//! which is what lets this library re-serialize its own artifact
//! byte-identically. Byte identity with another implementation's
//! writer is deliberately not claimed: two correct DEFLATE encoders
//! legitimately differ, and the reader accepts any valid stream,
//! which is what keeps every writer's artifacts readable here.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

use crate::codec::deflate::{zlib_compress, zlib_decompress};
use crate::error::{Error, Result};
use crate::evidence::{DeclaredLoss, Provenance};
use crate::flux::capture::{ByteSink, CHUNK_RECORDS, SessionBacking, read_varint, write_varint};
use crate::flux::remanence::image::{
    FluxImage, FluxImageBuilder, Hole, Magnetization, MediaFormFactor, MemorySource, OrbitKey,
    OrbitPoint, REMANENCE, TurnFraction, WriteWidths,
};
use crate::io::device::{self, AccessIntent, AccessMode};

/// The artifact's identity, spelled out where a reader will meet it.
pub(crate) const MAGIC: &[u8; 23] = b"REMANENCE_PHYSICAL_DISK";
/// The binary sentinel after the magic.
const SENTINEL: u8 = 0x1a;
/// The one layout this build reads and writes.
const VERSION: u8 = 1;
/// Magic, sentinel, version.
const HEADER_BYTES: usize = MAGIC.len() + 2;

/// The ceiling on a decompressed payload. The densest medium in the
/// device class packs a few tens of megabytes of points; a stream
/// claiming more than this is not an image of a disk.
const DECOMPRESSED_CAP: usize = 256 * 1024 * 1024;

/// The two tag bits riding low in every point's varint.
const STATES_MAGNETIZATION: u64 = 1;
const STATES_WIDTHS: u64 = 2;
const TAG_BITS: u32 = 2;

fn form_factor_code(form_factor: MediaFormFactor) -> u8 {
    match form_factor {
        MediaFormFactor::Inch8 => 0,
        MediaFormFactor::Inch525 => 1,
        MediaFormFactor::Inch35 => 2,
    }
}

fn form_factor_of(code: u8) -> Result<MediaFormFactor> {
    match code {
        0 => Ok(MediaFormFactor::Inch8),
        1 => Ok(MediaFormFactor::Inch525),
        2 => Ok(MediaFormFactor::Inch35),
        other => Err(Error::invalid_image(
            REMANENCE,
            format!(
                "artifact states a form factor code {other}, which this version has no reading of"
            ),
        )),
    }
}

fn magnetization_of(code: u8) -> Result<Magnetization> {
    Magnetization::from_code(u32::from(code)).ok_or_else(|| {
        Error::invalid_image(
            REMANENCE,
            format!("artifact states a magnetization code {code}, which names no state"),
        )
    })
}

/// Reads one zigzag-coded signed value.
fn read_signed(bytes: &[u8], at: usize) -> Result<(i64, usize)> {
    let (zigzag, used) = read_varint(REMANENCE, bytes, at)?;
    Ok((((zigzag >> 1) as i64) ^ -((zigzag & 1) as i64), used))
}

fn write_signed(out: &mut Vec<u8>, value: i64) {
    write_varint(out, ((value << 1) ^ (value >> 63)) as u64);
}

/// Whether `bytes` opens with this artifact's magic — the probe's
/// question, answered without reading further.
pub(crate) fn has_signature(bytes: &[u8]) -> bool {
    bytes.len() >= MAGIC.len() && &bytes[..MAGIC.len()] == MAGIC
}

/// Decodes one `.remanence` artifact held in memory into an image
/// whose orbit points live in an in-memory section backing.
pub(crate) fn from_bytes(bytes: &[u8]) -> Result<FluxImage> {
    let (image, sink, total) = decode(bytes, Vec::new(), CHUNK_RECORDS)?;
    let mut image = image;
    image.attach_backing(
        MemorySource(std::sync::Arc::new(sink)),
        total,
        crate::io::cache::DEFAULT_CACHE_BYTES,
    );
    Ok(image)
}

/// Decodes one `.remanence` artifact, streaming each orbit's packed
/// points into `sink`. Returns the image, the sink, and the backing's
/// total length; the caller attaches the backing.
pub(crate) fn decode<S: ByteSink>(
    bytes: &[u8],
    sink: S,
    chunk_records: usize,
) -> Result<(FluxImage, S, u64)> {
    if bytes.len() < HEADER_BYTES {
        return Err(Error::invalid_image(
            REMANENCE,
            format!(
                "too short to be a remanence artifact: {} bytes",
                bytes.len()
            ),
        ));
    }
    if !has_signature(bytes) {
        return Err(Error::invalid_image(
            REMANENCE,
            "artifact does not open with the remanence magic",
        ));
    }
    if bytes[MAGIC.len()] != SENTINEL {
        return Err(Error::invalid_image(
            REMANENCE,
            "the magic is not followed by the binary sentinel; this looks like the \
             text grammar the format used to have",
        ));
    }
    // A version this reader does not know is refused rather than
    // guessed at (P8): the layout is implicit, so a reader that
    // guesses does not fail, it misreads.
    let version = bytes[MAGIC.len() + 1];
    if version != VERSION {
        return Err(Error::invalid_image(
            REMANENCE,
            format!(
                "artifact states remanence version {version}; this reader knows \
                 version {VERSION}"
            ),
        ));
    }
    let payload = zlib_decompress(&bytes[HEADER_BYTES..], DECOMPRESSED_CAP).ok_or_else(|| {
        Error::invalid_image(
            REMANENCE,
            "the compressed payload does not decode as one whole zlib stream \
             checking to its own trailer",
        )
    })?;
    decode_payload(&payload, sink, chunk_records)
}

fn decode_payload<S: ByteSink>(
    payload: &[u8],
    sink: S,
    chunk_records: usize,
) -> Result<(FluxImage, S, u64)> {
    let mut at = 0;

    let form_factor =
        form_factor_of(*payload.first().ok_or_else(|| {
            Error::invalid_image(REMANENCE, "payload ends before the form factor")
        })?)?;
    at += 1;

    let (hole_count, used) = read_varint(REMANENCE, payload, at)?;
    at += used;
    let mut holes = Vec::new();
    for _ in 0..hole_count {
        let (center, used) = read_fraction(payload, at)?;
        at += used;
        let (extent, used) = read_fraction(payload, at)?;
        at += used;
        holes.push(Hole::new(center, extent)?);
    }

    // Orbits arrive grouped by surface, outermost first within each
    // group; the builder wants ascending key order, so a surface's
    // group is decoded whole and added reversed.
    let (surface_count, used) = read_varint(REMANENCE, payload, at)?;
    at += used;
    let mut builder = FluxImageBuilder::to_sink(
        form_factor,
        holes,
        Provenance::new(REMANENCE).note("decoded from a remanence artifact"),
        sink,
        chunk_records,
    )?;
    let mut last_surface: Option<u64> = None;
    for _ in 0..surface_count {
        let (surface, used) = read_varint(REMANENCE, payload, at)?;
        at += used;
        if last_surface.is_some_and(|last| surface <= last) {
            return Err(Error::invalid_image(
                REMANENCE,
                "surface blocks must ascend, each naming its surface once",
            ));
        }
        last_surface = Some(surface);
        let (orbit_count, used) = read_varint(REMANENCE, payload, at)?;
        at += used;
        let mut orbits: Vec<(OrbitKey, Vec<OrbitPoint>)> = Vec::new();
        for _ in 0..orbit_count {
            let (key, points, used) = read_orbit(payload, at, surface)?;
            at += used;
            orbits.push((key, points));
        }
        // Outermost-first in the file; the builder takes them
        // innermost-first so keys ascend.
        for (key, points) in orbits.into_iter().rev() {
            builder.add_orbit(key, &points)?;
        }
    }

    if at != payload.len() {
        return Err(Error::invalid_image(
            REMANENCE,
            format!(
                "unexpected trailing content after the disk: {} bytes",
                payload.len() - at
            ),
        ));
    }
    let (image, sink, total) = builder.seal()?;
    Ok((image, sink, total))
}

fn read_fraction(payload: &[u8], at: usize) -> Result<(TurnFraction, usize)> {
    let mut cursor = at;
    let (numerator, used) = read_signed(payload, cursor)?;
    cursor += used;
    let (denominator, used) = read_varint(REMANENCE, payload, cursor)?;
    cursor += used;
    let numerator = u64::try_from(numerator)
        .map_err(|_| Error::invalid_image(REMANENCE, "a hole's angle cannot be negative"))?;
    Ok((TurnFraction::new(numerator, denominator)?, cursor - at))
}

fn read_orbit(
    payload: &[u8],
    at: usize,
    surface: u64,
) -> Result<(OrbitKey, Vec<OrbitPoint>, usize)> {
    let mut cursor = at;
    let (radius, used) = read_varint(REMANENCE, payload, cursor)?;
    cursor += used;
    let key = OrbitKey::new(surface, radius)?;
    let (point_count, used) = read_varint(REMANENCE, payload, cursor)?;
    cursor += used;

    let mut points = Vec::new();
    let mut angle: u64 = 0;
    let mut last_sense: Option<Magnetization> = None;
    for _ in 0..point_count {
        let (tagged, used) = read_varint(REMANENCE, payload, cursor)?;
        cursor += used;
        let tag = tagged & ((1 << TAG_BITS) - 1);
        angle += tagged >> TAG_BITS;
        let sense = if tag & STATES_MAGNETIZATION != 0 {
            let code = *payload.get(cursor).ok_or_else(|| {
                Error::invalid_image(REMANENCE, "payload ends inside a stated magnetization")
            })?;
            cursor += 1;
            magnetization_of(code)?
        } else {
            last_sense
                .and_then(|sense| sense.opposite())
                .ok_or_else(|| {
                    Error::invalid_image(
                        REMANENCE,
                        format!(
                            "a point with no stated sense needs a preceding coherent one \
                             to alternate from, and there is none before angle {angle}"
                        ),
                    )
                })?
        };
        let widths = if tag & STATES_WIDTHS != 0 {
            let (plateau, used) = read_varint(REMANENCE, payload, cursor)?;
            cursor += used;
            let (guard, used) = read_varint(REMANENCE, payload, cursor)?;
            cursor += used;
            Some(WriteWidths::new(plateau, guard)?)
        } else {
            None
        };
        points.push(OrbitPoint::stating(angle, sense, widths)?);
        last_sense = if sense.is_coherent() {
            Some(sense)
        } else {
            None
        };
    }
    Ok((key, points, cursor - at))
}

/// Encodes one image as a complete `.remanence` artifact.
pub(crate) fn to_bytes(image: &FluxImage) -> Result<Vec<u8>> {
    let mut payload = Vec::new();
    payload.push(form_factor_code(image.form_factor()));

    write_varint(&mut payload, image.holes().len() as u64);
    for hole in image.holes() {
        write_signed(&mut payload, hole.center_angle().numerator() as i64);
        write_varint(&mut payload, hole.center_angle().denominator());
        write_signed(&mut payload, hole.angular_extent().numerator() as i64);
        write_varint(&mut payload, hole.angular_extent().denominator());
    }

    // Grouped by surface, each block naming its surface rather than
    // being found by position; within a surface, outermost first.
    let mut surfaces: Vec<u64> = image.orbits().map(|orbit| orbit.key().surface()).collect();
    surfaces.dedup();
    write_varint(&mut payload, surfaces.len() as u64);
    for surface in surfaces {
        write_varint(&mut payload, surface);
        let held: Vec<_> = image
            .orbits()
            .filter(|orbit| orbit.key().surface() == surface)
            .collect();
        write_varint(&mut payload, held.len() as u64);
        for orbit in held.into_iter().rev() {
            write_varint(&mut payload, orbit.key().radius_microns());
            write_varint(&mut payload, orbit.points());
            let points = image.points(orbit)?;
            let mut previous_angle: u64 = 0;
            let mut last_sense: Option<Magnetization> = None;
            for point in &points {
                let derivable = point.magnetization().is_coherent()
                    && last_sense.and_then(|sense| sense.opposite()) == Some(point.magnetization());
                let tag = if derivable { 0 } else { STATES_MAGNETIZATION }
                    | if point.states_widths() {
                        STATES_WIDTHS
                    } else {
                        0
                    };
                write_varint(
                    &mut payload,
                    ((point.angle() - previous_angle) << TAG_BITS) | tag,
                );
                if !derivable {
                    payload.push(point.magnetization().code() as u8);
                }
                if let Some(widths) = point.widths() {
                    write_varint(&mut payload, widths.plateau_microns());
                    write_varint(&mut payload, widths.guard_microns());
                }
                previous_angle = point.angle();
                last_sense = if point.magnetization().is_coherent() {
                    Some(point.magnetization())
                } else {
                    None
                };
            }
        }
    }

    let mut out = Vec::with_capacity(HEADER_BYTES + payload.len() / 4);
    out.extend_from_slice(MAGIC);
    out.push(SENTINEL);
    out.push(VERSION);
    out.extend_from_slice(&zlib_compress(&payload));
    Ok(out)
}

// ----------------------------------------------------- the public root

/// What writing an image into a `.remanence` artifact carried.
///
/// The account is P29's, and for this destination it is empty by
/// construction: the remanence artifact is the model's own, so it
/// carries every fact the image holds. An empty account is the claim
/// that nothing was left behind, not an account nobody assembled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FluxWriteReport {
    /// Where the artifact was written.
    pub path: String,
    /// The artifact's size on storage.
    pub artifact_bytes: u64,
    pub orbits: u64,
    /// Every point across every orbit the artifact carries.
    pub points: u64,
    /// What the destination did not carry (P29) — empty for this
    /// format, always.
    pub declared_loss: Vec<DeclaredLoss>,
}

impl FluxImage {
    /// Opens the `.remanence` artifact at `path` with the stated
    /// default session cache bound ([`crate::DEFAULT_CACHE_BYTES`]).
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::open_with_cache(path, crate::DEFAULT_CACHE_BYTES)
    }

    /// Opens the artifact under a caller-declared cache bound (P27): at
    /// most `cache_bytes` of the decoded image stays resident. The
    /// bound narrows the working set; it never refuses service.
    ///
    /// Opening claims the file (P7) — writes denied to every other
    /// process — decodes the whole artifact once into private session
    /// storage, and holds the claim until the image is dropped, so what
    /// the image was read from cannot change beneath it.
    pub fn open_with_cache(path: impl AsRef<Path>, cache_bytes: u64) -> Result<Self> {
        let path = path.as_ref();
        let claimed = device::open_declared(path, AccessIntent::Read)?;
        let length = claimed
            .metadata()
            .map_err(|error| Error::io(format!("cannot size '{}': {error}", path.display())))?
            .len();
        // The payload is one zlib stream and is inflated whole, so the
        // artifact is read whole; a file too large to be an image of a
        // disk is refused before any of it is allocated.
        let held = usize::try_from(length)
            .ok()
            .filter(|&held| held <= DECOMPRESSED_CAP)
            .ok_or_else(|| {
                Error::invalid_image(
                    REMANENCE,
                    format!(
                        "'{}' is {length} bytes, which is larger than any image of a \
                         disk in this device class",
                        path.display()
                    ),
                )
            })?;
        let mut bytes = vec![0u8; held];
        device::read_exact_at(&claimed, 0, &mut bytes)
            .map_err(|error| Error::io(format!("cannot read '{}': {error}", path.display())))?;

        let (mut image, backing, total) = decode(&bytes, SessionBacking::create()?, CHUNK_RECORDS)?;
        image.attach_backing(backing.into_source(), total, cache_bytes);
        image.attach_artifact(path.to_path_buf(), claimed, AccessMode::ReadOnly);
        Ok(image)
    }

    /// Writes this image into a new `.remanence` artifact at `path`,
    /// and reports what the artifact carried.
    ///
    /// The image is untouched. An existing destination is a named
    /// refusal rather than an overwrite, and an interruption leaves the
    /// destination absent rather than half an artifact (P6, P7, P9).
    ///
    /// The bytes are deterministic — the same image spells the same
    /// artifact, every time. Byte identity with another
    /// implementation's writer is deliberately not claimed: two correct
    /// DEFLATE encoders legitimately differ, and this reader accepts
    /// any valid stream.
    pub fn write(&self, path: impl AsRef<Path>) -> Result<FluxWriteReport> {
        write_new_artifact(self, path.as_ref())
    }
}

fn write_new_artifact(image: &FluxImage, path: &Path) -> Result<FluxWriteReport> {
    if path.try_exists().unwrap_or(false) {
        return Err(Error::io(format!(
            "cannot write '{}': something is already there, and a destination this \
             library did not create is never overwritten",
            path.display()
        )));
    }
    let artifact = to_bytes(image)?;

    let staging = staging_path(path);
    let file = device::create_claimed(&staging)?;
    // The bytes are on the medium before the name exists, so what the
    // destination names is either the whole artifact or nothing.
    let built = device::write_all_at(&file, 0, &artifact)
        .map_err(|error| Error::io(format!("cannot write '{}': {error}", staging.display())))
        .and_then(|()| {
            file.sync_all().map_err(|error| {
                Error::io(format!(
                    "cannot commit '{}' to storage: {error}",
                    staging.display()
                ))
            })
        });
    drop(file);
    if let Err(error) = built {
        let _ = std::fs::remove_file(&staging);
        return Err(error);
    }
    std::fs::rename(&staging, path).map_err(|error| {
        let _ = std::fs::remove_file(&staging);
        Error::io(format!(
            "cannot put the written artifact in place at '{}': {error}",
            path.display()
        ))
    })?;

    Ok(FluxWriteReport {
        path: path.display().to_string(),
        artifact_bytes: artifact.len() as u64,
        orbits: image.orbit_count() as u64,
        points: image.orbits().map(|orbit| orbit.points()).sum(),
        // Nothing: this is the model's own artifact.
        declared_loss: Vec::new(),
    })
}

/// Where the artifact is built: beside its destination, so moving it
/// into place is a rename within one filesystem rather than a copy.
fn staging_path(destination: &Path) -> PathBuf {
    let name = destination.file_name().map_or_else(
        || "artifact".to_owned(),
        |name| name.to_string_lossy().into_owned(),
    );
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.subsec_nanos());
    destination.with_file_name(format!(
        ".{name}.remanence-{}-{nonce}.part",
        std::process::id()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flux::remanence::image::validate_orbit_points;

    /// The format's worked example: one hole at 3/8 of a turn, one
    /// orbit at 57,150 µm opening POSITIVE with the standard geometry,
    /// alternating to NEGATIVE 500 divisions on.
    const EXAMPLE_PAYLOAD: [u8; 21] = [
        0x01, // form factor: 5.25-inch
        0x01, // one hole
        0x06, 0x08, // 3/8 of a turn
        0x02, 0x32, // 1/50 extent
        0x01, // one surface
        0x00, // surface 0
        0x01, // one orbit
        0xbe, 0xbe, 0x03, // centre radius 57150
        0x02, // two points
        0x03, 0x00, 0xca, 0x02, 0xb0, 0x03, // +0, POSITIVE, plateau 330, guard 432
        0xd0, 0x0f, // +500, nothing stated: alternates to NEGATIVE
    ];

    fn example_artifact() -> Vec<u8> {
        let mut artifact = Vec::new();
        artifact.extend_from_slice(MAGIC);
        artifact.push(SENTINEL);
        artifact.push(VERSION);
        artifact.extend_from_slice(&zlib_compress(&EXAMPLE_PAYLOAD));
        artifact
    }

    #[test]
    fn the_worked_example_decodes_to_the_stated_disk() {
        let image = from_bytes(&example_artifact()).expect("the example decodes");
        assert_eq!(image.form_factor(), MediaFormFactor::Inch525);
        assert_eq!(image.holes().len(), 1);
        let hole = image.holes()[0];
        assert_eq!(
            (
                hole.center_angle().numerator(),
                hole.center_angle().denominator()
            ),
            (3, 8)
        );
        assert_eq!(
            (
                hole.angular_extent().numerator(),
                hole.angular_extent().denominator()
            ),
            (1, 50)
        );
        assert_eq!(image.orbit_count(), 1);
        let orbit = image.orbits().next().expect("one orbit").clone();
        assert_eq!(orbit.key().surface(), 0);
        assert_eq!(orbit.key().radius_microns(), 57_150);
        let points = image.points(&orbit).expect("the points decode");
        assert_eq!(points.len(), 2);
        assert_eq!(points[0].angle(), 0);
        assert_eq!(points[0].magnetization(), Magnetization::Positive);
        let widths = points[0].widths().expect("the first point states widths");
        assert_eq!(widths.plateau_microns(), 330);
        assert_eq!(widths.guard_microns(), 432);
        assert_eq!(points[1].angle(), 500);
        assert_eq!(points[1].magnetization(), Magnetization::Negative);
        assert!(points[1].widths().is_none());
    }

    #[test]
    fn the_header_is_gated_before_anything_is_believed() {
        let artifact = example_artifact();

        let mut wrong_magic = artifact.clone();
        wrong_magic[0] = b'X';
        assert!(from_bytes(&wrong_magic).is_err());

        let mut wrong_sentinel = artifact.clone();
        wrong_sentinel[MAGIC.len()] = b'\n';
        assert!(from_bytes(&wrong_sentinel).is_err());

        let mut wrong_version = artifact.clone();
        wrong_version[MAGIC.len() + 1] = 2;
        assert!(from_bytes(&wrong_version).is_err());

        assert!(from_bytes(&artifact[..10]).is_err());
    }

    #[test]
    fn trailing_payload_content_is_refused() {
        let mut padded = EXAMPLE_PAYLOAD.to_vec();
        padded.push(0);
        let mut artifact = Vec::new();
        artifact.extend_from_slice(MAGIC);
        artifact.push(SENTINEL);
        artifact.push(VERSION);
        artifact.extend_from_slice(&zlib_compress(&padded));
        let refusal = from_bytes(&artifact).expect_err("a payload saying more than the disk");
        assert!(
            refusal.to_string().contains("trailing content"),
            "the refusal names the trailing content: {refusal}"
        );
    }

    #[test]
    fn an_image_round_trips_and_reserializes_identically() {
        let image = from_bytes(&example_artifact()).expect("the example decodes");
        let encoded = to_bytes(&image).expect("the image encodes");
        let again = from_bytes(&encoded).expect("our own artifact decodes");

        assert_eq!(again.form_factor(), image.form_factor());
        assert_eq!(again.holes(), image.holes());
        assert_eq!(again.orbit_count(), image.orbit_count());
        for (mine, theirs) in image.orbits().zip(again.orbits()) {
            assert_eq!(mine.key(), theirs.key());
            assert_eq!(
                image.points(mine).expect("points decode"),
                again.points(theirs).expect("points decode")
            );
        }

        // Determinism: the same image spells the same bytes.
        assert_eq!(to_bytes(&again).expect("the image encodes"), encoded);
    }

    #[test]
    fn elision_survives_a_splice_and_a_reopening() {
        // An orbit exercising every case the sense byte survives in:
        // the first coherent point, a width-stating splice repeating
        // its predecessor's polarity, and a reopening after an
        // unaligned span.
        let widths = WriteWidths::new(330, 432).unwrap();
        let narrower = WriteWidths::new(300, 400).unwrap();
        let points = vec![
            OrbitPoint::stating(100, Magnetization::Positive, Some(widths)).unwrap(),
            OrbitPoint::new(200, Magnetization::Negative).unwrap(),
            OrbitPoint::stating(300, Magnetization::Negative, Some(narrower)).unwrap(),
            OrbitPoint::new(400, Magnetization::Unaligned).unwrap(),
            OrbitPoint::new(500, Magnetization::Negative).unwrap(),
            OrbitPoint::new(600, Magnetization::Positive).unwrap(),
        ];
        validate_orbit_points(&points).expect("the exercise orbit is valid");

        let mut builder = FluxImageBuilder::in_memory(
            MediaFormFactor::Inch525,
            Vec::new(),
            Provenance::new(REMANENCE).note("built by a unit test"),
        )
        .unwrap();
        builder
            .add_orbit(OrbitKey::new(0, 57_150).unwrap(), &points)
            .unwrap();
        let (mut image, sink, total) = builder.seal().unwrap();
        image.attach_backing(
            MemorySource(std::sync::Arc::new(sink)),
            total,
            crate::io::cache::DEFAULT_CACHE_BYTES,
        );

        let encoded = to_bytes(&image).expect("the image encodes");
        let decoded = from_bytes(&encoded).expect("our own artifact decodes");
        let orbit = decoded.orbits().next().expect("one orbit").clone();
        assert_eq!(decoded.points(&orbit).expect("points decode"), points);
    }
}
