// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The MAME floppy image (F73): cell transitions around a revolution,
//! decoded into the served flux medium.
//!
//! **This is a flux artifact, not a sector one.** A track is a run of
//! transitions expressed as *time deltas* around one revolution, each a
//! fraction of a turn rather than a count of bytes — so it loads into a
//! medium and reaches sectors the way a capture does, through the
//! family's declared channel and codec, rather than through a byte
//! device (P13).
//!
//! **The turn is the unit, and the profile supplies the clock.** MFI
//! divides a revolution into two hundred million parts and states each
//! transition's distance from the last in them. What that is in the
//! family's own reference cycles is a projection, and the one division
//! it needs states its remainder rather than swallowing it (P29): a
//! delta that does not land on a whole cycle is counted into the
//! declared-loss account.
//!
//! **What the container does not carry is not invented.** MFI states no
//! rate, no encoding and no drive; the profile the caller's declaration
//! names supplies all three, and the artifact's own geometry is checked
//! against it rather than overriding it. A recording of eighty
//! cylinders declared as a forty-cylinder family is refused showing both
//! numbers.

use crate::codec::deflate::zlib_decompress;
use crate::error::{Error, Result};
use crate::evidence::{LossAccount, Provenance};
use crate::flux::capture::TimeBase;
use crate::flux::drive_profile::DriveProfile;
use crate::flux::medium::{
    Derivation, FluxMedium, LocationKey, MediumBuilder, OriginRule, OriginStatement, Pulse,
    RotationalFrame, Strength,
};
use crate::io::source::ImageSource;

/// Every MAME floppy image opens with this.
pub(crate) const MFI_MAGIC: &[u8] = b"MAMEFLOPPYIMAGE\0";

/// The parts MFI divides one revolution into.
const TURN: u64 = 200_000_000;

/// The fixed header: the signature, the two counts, and the form-factor
/// and variant strings.
const HEADER_BYTES: usize = 32;

/// One track-table entry: offset, compressed size, uncompressed size and
/// the write splice's angular position.
const ENTRY_BYTES: usize = 16;

/// The largest artifact this reader decodes (P27). MFI is a floppy
/// format; the bound is generous for one and stated rather than assumed.
const MFI_BOUND: u64 = 64 * 1024 * 1024;

/// The cell types MFI's top nibble states.
///
/// Only the transition record has been read against a real artifact, so
/// it is the only one claimed. The unmagnetized and damaged records are
/// enumerated here to be *refused by name* rather than guessed at: what
/// they mean for a served medium is a real question — whether such a
/// region is absence, weakness, or a refusal of its own — and answering
/// it from the format's shape alone would be inventing evidence.
const TRANSITION: u32 = 0;

fn refuse(reason: impl Into<String>) -> Error {
    Error::invalid_image("mfi", reason)
}

/// What the decode observed, for the load's account (P4).
pub(crate) struct MfiReport {
    pub(crate) evidence: Vec<String>,
    pub(crate) declared_loss: Vec<crate::evidence::DeclaredLoss>,
}

/// Reads one 32-bit little-endian field.
fn le32(bytes: &[u8], at: usize) -> u64 {
    u64::from(u32::from_le_bytes([
        bytes[at],
        bytes[at + 1],
        bytes[at + 2],
        bytes[at + 3],
    ]))
}

/// Decodes an MFI artifact into the medium of the declared family.
pub(crate) fn decode(
    source: &ImageSource,
    named: &str,
    profile: &'static DriveProfile,
    cache_bytes: u64,
) -> Result<(FluxMedium, MfiReport)> {
    let length = source.len();
    if length > MFI_BOUND {
        return Err(Error::categorized_image(
            crate::error::ErrorCategory::Unsupported,
            "mfi",
            format!("{named} is {length} bytes and the MFI reader is bounded to {MFI_BOUND}"),
        ));
    }
    let mut bytes = vec![0u8; length as usize];
    source.read_at(0, &mut bytes)?;

    if !bytes.starts_with(MFI_MAGIC) {
        return Err(refuse(format!(
            "{named} does not open with the MAME floppy image signature"
        )));
    }
    if bytes.len() < HEADER_BYTES {
        return Err(refuse(format!("{named} is shorter than its own header")));
    }

    let cylinders = le32(&bytes, 16);
    let heads = le32(&bytes, 20);
    let form = String::from_utf8_lossy(&bytes[24..32])
        .trim_end_matches('\0')
        .trim()
        .to_owned();
    if cylinders == 0 || heads == 0 {
        return Err(refuse(format!(
            "{named} states {cylinders} cylinder(s) of {heads} head(s), which is no \
             recording at all"
        )));
    }

    // The family's declaration is what the artifact is checked against;
    // neither overrides the other, and a disagreement is refused with
    // both numbers rather than one silently winning.
    let declared_surfaces = u64::from(profile.surfaces.recorded);
    if heads != declared_surfaces {
        return Err(refuse(format!(
            "{named} records {heads} head(s) and the '{}' family declares {declared_surfaces}; \
             a recording is read by the family it was made on",
            profile.id
        )));
    }

    let table_end = HEADER_BYTES + (cylinders * heads) as usize * ENTRY_BYTES;
    if bytes.len() < table_end {
        return Err(refuse(format!(
            "{named} states {cylinders}x{heads} tracks, whose table runs to {table_end} \
             and the artifact holds {}",
            bytes.len()
        )));
    }

    let frame = RotationalFrame::new(
        profile.id,
        TimeBase::new(profile.id, profile.rotation.reference_clock, 1)?,
        profile.rotation.cycles_per_rotation,
        OriginStatement::new(
            OriginRule::Index,
            Provenance::new("mfi")
                .note("MFI places its transitions around a revolution from the index"),
        ),
    )?;

    let mut builder = MediumBuilder::new(
        profile.id,
        profile.media,
        frame,
        Derivation::SelectedAndProjected,
        Provenance::new("mfi").note(format!(
            "decoded from a MAME floppy image of {cylinders} cylinder(s) by {heads} \
             head(s), form factor '{form}'"
        )),
        crate::flux::capture::SessionBacking::create()?,
    )?;

    let mut loss = LossAccount::new();
    let mut inexact = 0u64;
    let mut transitions = 0u64;
    let mut unformatted = 0u64;

    for index in 0..(cylinders * heads) as usize {
        let entry = HEADER_BYTES + index * ENTRY_BYTES;
        let offset = le32(&bytes, entry) as usize;
        let compressed = le32(&bytes, entry + 4) as usize;
        let uncompressed = le32(&bytes, entry + 8) as usize;

        // MAME lays its tracks out cylinder-major.
        let cylinder = index as u64 / heads;
        let head = index as u64 % heads;

        if compressed == 0 || uncompressed == 0 {
            // A track the artifact holds nothing for is unformatted, and
            // saying so is different from claiming an empty recording.
            unformatted += 1;
            continue;
        }
        if offset + compressed > bytes.len() {
            return Err(refuse(format!(
                "the track at cylinder {cylinder} head {head} states its data at \
                 {offset}..{} and {named} holds {}",
                offset + compressed,
                bytes.len()
            )));
        }
        let raw = zlib_decompress(&bytes[offset..offset + compressed], uncompressed).ok_or_else(
            || {
                refuse(format!(
                    "the track at cylinder {cylinder} head {head} does not decompress to \
                     the {uncompressed} bytes it states"
                ))
            },
        )?;
        if raw.len() % 4 != 0 {
            return Err(refuse(format!(
                "the track at cylinder {cylinder} head {head} decompresses to {} bytes, \
                 which are not whole cell records",
                raw.len()
            )));
        }

        let mut pulses = Vec::with_capacity(raw.len() / 4);
        let mut at_units = 0u64;
        for record in raw.chunks_exact(4) {
            let value = u32::from_le_bytes([record[0], record[1], record[2], record[3]]);
            let kind = value >> 28;
            if kind != TRANSITION {
                return Err(refuse(format!(
                    "the track at cylinder {cylinder} head {head} carries a cell record of \
                     type {kind}; this release reads the transition record and refuses the \
                     others rather than deciding what an unmagnetized or damaged region \
                     means for a served medium"
                )));
            }
            at_units += u64::from(value & 0x0fff_ffff);
            if at_units > TURN {
                return Err(refuse(format!(
                    "the transitions of cylinder {cylinder} head {head} run past one \
                     revolution, reaching {at_units} of {TURN}"
                )));
            }
            // The one division: the turn projected onto the family's own
            // circle. Its remainder is observed rather than discarded.
            let scaled = at_units * profile.rotation.cycles_per_rotation;
            if scaled % TURN != 0 {
                inexact += 1;
            }
            pulses.push(Pulse::new(scaled / TURN, Strength::certain(2)));
            transitions += 1;
        }

        builder.add_location(
            LocationKey::new(profile.id, cylinder, head),
            &pulses,
            &[],
            Provenance::new("mfi").note(format!(
                "{} transitions around the revolution, projected onto the family's \
                 {} cycle circle",
                pulses.len(),
                profile.rotation.cycles_per_rotation
            )),
        )?;
    }

    if unformatted > 0 {
        loss.add(
            "unformatted-track",
            "tracks the artifact holds no cells for, which carry no recording rather than \
             an empty one",
            unformatted,
        );
    }
    if inexact > 0 {
        loss.add(
            "projected-angle",
            "transitions whose angle does not fall on a whole cycle of the family's \
             circle, placed at the cycle below and counted rather than silently rounded",
            inexact,
        );
    }

    let evidence = vec![
        format!(
            "MAME floppy image: {cylinders} cylinder(s) by {heads} head(s), form factor \
             '{form}'"
        ),
        format!(
            "{transitions} transition(s) read, each stated as a fraction of a revolution \
             and projected onto the '{}' family's {} cycle circle",
            profile.id, profile.rotation.cycles_per_rotation
        ),
        "the container states no rate, encoding or drive; all three are the declared \
         family's, and the artifact's geometry was checked against it rather than \
         overriding it"
            .to_owned(),
    ];

    let (mut medium, sink, total) = builder.seal()?;
    medium.attach_backing(Box::new(sink.into_source()), total, cache_bytes);

    Ok((
        medium,
        MfiReport {
            evidence,
            declared_loss: loss.into_entries(),
        },
    ))
}
