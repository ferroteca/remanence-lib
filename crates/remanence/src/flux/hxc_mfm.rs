// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The HxC Floppy Emulator's `.mfm` container (F77).
//!
//! **This container sits at the bit tier, not below it.** An MFI track
//! is transitions around a revolution — timing the recording actually
//! had. An HxC MFM track is *already-framed cells*: MFM bits at one
//! declared rate, laid down before the file existed. The distinction is
//! the reason to read both, so it is carried rather than levelled: this
//! medium's [`Derivation`] is [`Derivation::Synthetic`], which is the
//! model's own word for a layer synthesized downward from a higher one
//! (P13). The pulses beneath the cells are this library's restatement of
//! the cells, never evidence the file supplied, and nothing above may
//! present them as recovered timing.
//!
//! **What the container does not carry is stated rather than supplied.**
//! An HxC MFM file holds no weak region, no density variation and no
//! second observation of a location. Every cell is therefore certain,
//! every location is read once, and the absences are declared on the
//! medium rather than filled in with plausible values.
//!
//! **The rate is the container's and the family's, and a mismatch is
//! refused.** The file states a bit rate and an RPM; the profile states
//! a cell rate and a rotation. Where they disagree the artifact is
//! refused by name showing both numbers, rather than clocked at a rate
//! nobody stated.
//!
//! The artifact is read; nothing is written.

use crate::error::{Error, Result};
use crate::evidence::{DeclaredLoss, LossAccount, Provenance};
use crate::flux::capture::TimeBase;
use crate::flux::drive_profile::DriveProfile;
use crate::flux::medium::{
    Derivation, FluxMedium, LocationKey, MediumBuilder, OriginRule, OriginStatement, Pulse,
    RotationalFrame, Strength,
};
use crate::io::source::ImageSource;

/// What the container opens with: the name, then its terminator.
const MFM_MAGIC: &[u8] = b"HXCMFM\0";

/// The header is the signature and six declarations.
const HEADER_BYTES: usize = 19;

/// Each track states its number, side, size and where its cells are.
const ENTRY_BYTES: usize = 11;

/// The reader is bounded like every other (P27). A double-sided
/// 96 TPI recording at 500 kbit/s is about a megabyte of cells, so this
/// admits an artifact several times larger than any this release's
/// families record and still refuses one that would have to be held
/// whole without saying so.
const MFM_BOUND: u64 = 64 * 1024 * 1024;

fn refuse(reason: impl Into<String>) -> Error {
    Error::invalid_image("hxc-mfm", reason)
}

fn le16(bytes: &[u8], at: usize) -> u64 {
    u64::from(u16::from_le_bytes([bytes[at], bytes[at + 1]]))
}

fn le32(bytes: &[u8], at: usize) -> u64 {
    u64::from(u32::from_le_bytes([
        bytes[at],
        bytes[at + 1],
        bytes[at + 2],
        bytes[at + 3],
    ]))
}

/// What the container declared, and what reading it cost.
///
/// The declarations themselves — track and side counts, rate, rotation,
/// interface mode — are stated in the evidence rather than repeated as
/// fields, because evidence is where a caller reads them and a second
/// copy would be a second thing to keep true.
#[derive(Debug, Clone)]
pub(crate) struct HxcMfmReport {
    pub(crate) evidence: Vec<String>,
    pub(crate) declared_loss: Vec<DeclaredLoss>,
}

/// Reads an HxC MFM container into a medium whose cells are the
/// container's own.
pub(crate) fn decode(
    source: &ImageSource,
    named: &str,
    profile: &'static DriveProfile,
    cache_bytes: u64,
) -> Result<(FluxMedium, HxcMfmReport)> {
    let length = source.len();
    if length > MFM_BOUND {
        return Err(Error::categorized_image(
            crate::error::ErrorCategory::Unsupported,
            "hxc-mfm",
            format!("{named} is {length} bytes and the HxC MFM reader is bounded to {MFM_BOUND}"),
        ));
    }
    let mut bytes = vec![0u8; length as usize];
    source.read_at(0, &mut bytes)?;

    if !bytes.starts_with(MFM_MAGIC) {
        return Err(refuse(format!(
            "{named} does not open with the HxC MFM signature"
        )));
    }
    if bytes.len() < HEADER_BYTES {
        return Err(refuse(format!("{named} is shorter than its own header")));
    }

    let tracks = le16(&bytes, 7);
    let sides = u64::from(bytes[9]);
    let rpm = le16(&bytes, 10);
    let bitrate_kbps = le16(&bytes, 12);
    let interface_type = bytes[14];
    let list_at = le32(&bytes, 15) as usize;

    if tracks == 0 || sides == 0 {
        return Err(refuse(format!(
            "{named} states {tracks} track(s) by {sides} side(s), which describes no \
             recording"
        )));
    }

    // The container's own declarations, checked against the family
    // rather than overriding it (P12). A drive reads the family it is,
    // and an artifact that is not of that family is refused by name
    // showing both numbers rather than read at a rate nobody stated.
    if sides != u64::from(profile.surfaces.recorded) {
        return Err(refuse(format!(
            "{named} records {sides} side(s) and the family '{}' records {}; a \
             recording of one is not a recording of the other",
            profile.id, profile.surfaces.recorded
        )));
    }
    check_rate(named, profile, bitrate_kbps)?;
    let cell_cycles = check_rotation(named, profile, rpm)?;

    let entries = (tracks * sides) as usize;
    let list_end = list_at
        .checked_add(entries * ENTRY_BYTES)
        .ok_or_else(|| refuse(format!("{named} states a track list past any address")))?;
    if list_end > bytes.len() {
        return Err(refuse(format!(
            "{named} states its track list at {list_at}..{list_end} and holds {} bytes",
            bytes.len()
        )));
    }

    let frame = RotationalFrame::new(
        profile.id,
        TimeBase::new(profile.id, profile.rotation.reference_clock, 1)?,
        profile.rotation.cycles_per_rotation,
        OriginStatement::new(
            OriginRule::Index,
            Provenance::new("hxc-mfm")
                .note("HxC MFM lays a track's cells down from the index datum"),
        ),
    )?;

    let mut builder = MediumBuilder::new(
        profile.id,
        profile.media,
        frame,
        // The cells are the container's; the transitions beneath them
        // are this library's restatement of the cells and are declared
        // as such (P13). Nothing above may present them as recovered
        // timing, because the file never held any.
        Derivation::Synthetic,
        Provenance::new("hxc-mfm").note(format!(
            "decoded from an HxC MFM container of {tracks} track(s) by {sides} side(s) \
             at {bitrate_kbps} kbit/s and {rpm} RPM; the cells are the container's and \
             the transitions beneath them are this reader's restatement of them"
        )),
        crate::flux::capture::SessionBacking::create()?,
    )?;

    let mut loss = LossAccount::new();
    let mut cells_read = 0u64;
    let mut transitions = 0u64;
    let mut unformatted = 0u64;
    let mut overrun = 0u64;
    let cells_per_turn = profile.rotation.cycles_per_rotation / cell_cycles;

    for index in 0..entries {
        let entry = list_at + index * ENTRY_BYTES;
        let track_number = le16(&bytes, entry);
        let side_number = u64::from(bytes[entry + 2]);
        let size = le32(&bytes, entry + 3) as usize;
        let at = le32(&bytes, entry + 7) as usize;

        if track_number >= tracks || side_number >= sides {
            return Err(refuse(format!(
                "{named} states a track numbered {track_number} side {side_number} \
                 where it declares {tracks} track(s) by {sides} side(s)"
            )));
        }
        if size == 0 {
            // A track the container holds no cells for is unformatted,
            // and saying so is different from claiming an empty
            // recording.
            unformatted += 1;
            continue;
        }
        let end = at
            .checked_add(size)
            .ok_or_else(|| refuse(format!("{named} states a track past any address")))?;
        if end > bytes.len() {
            return Err(refuse(format!(
                "the track numbered {track_number} side {side_number} states its cells \
                 at {at}..{end} and {named} holds {} bytes",
                bytes.len()
            )));
        }

        // The cells, most significant bit first — which is the order the
        // container writes them and the order the encoding reads them.
        let mut pulses = Vec::new();
        let mut cell = 0u64;
        for byte in &bytes[at..end] {
            for shift in (0..8).rev() {
                if byte >> shift & 1 == 1 {
                    pulses.push(Pulse::new(cell * cell_cycles, Strength::certain(2)));
                    transitions += 1;
                }
                cell += 1;
            }
        }
        cells_read += cell;

        // A track longer than the circle is the container's business and
        // not this reader's to trim: the cells past one revolution are
        // counted and declared rather than dropped in silence.
        if cell > cells_per_turn {
            overrun += 1;
            loss.add(
                "hxc-mfm.track-longer-than-the-circle",
                "the container states more cells for a track than the family's circle \
                 holds; the cells are read as stated and the circle is the family's",
                cell - cells_per_turn,
            );
        }

        builder.add_location(
            LocationKey::new(profile.id, track_number, side_number),
            &pulses,
            &[],
            Provenance::new("hxc-mfm").note(format!(
                "{cell} cells as the container states them, {} of them transitions",
                pulses.len()
            )),
        )?;
    }

    if unformatted > 0 {
        loss.add(
            "hxc-mfm.track-holds-no-cells",
            "the container states these tracks and holds no cells for them; they are \
             absent from the medium rather than present and empty",
            unformatted,
        );
    }

    let (mut medium, sink, total) = builder.seal()?;
    medium.attach_backing(Box::new(sink.into_source()), total, cache_bytes);
    let report = HxcMfmReport {
        evidence: vec![
            format!(
                "an HxC MFM container of {tracks} track(s) by {sides} side(s), interface \
                 mode {interface_type}, stating {bitrate_kbps} kbit/s at {rpm} RPM"
            ),
            format!(
                "{cells_read} cells read as the container states them, {transitions} of \
                 them transitions"
            ),
            "the container carries no weak region, no density variation and no second \
             observation of a location; every cell is certain, every location is read \
             once, and none of those absences is filled in"
                .to_owned(),
            "the cells are the container's own and the transitions beneath them are this \
             reader's restatement of them, declared synthetic rather than presented as \
             recovered timing"
                .to_owned(),
        ],
        declared_loss: loss.into_entries(),
    };
    let _ = overrun;
    Ok((medium, report))
}

/// Checks the container's declared bit rate against the family's cell
/// rate, in the family's own terms.
fn check_rate(named: &str, profile: &'static DriveProfile, bitrate_kbps: u64) -> Result<()> {
    let zone = profile
        .density
        .first()
        .ok_or_else(|| refuse(format!("the family '{}' declares no cell rate", profile.id)))?;
    if zone.rate_denominator == 0 {
        return Err(refuse(format!(
            "the family '{}' declares a cell rate over zero",
            profile.id
        )));
    }
    let declared = zone.rate_numerator / zone.rate_denominator;
    if bitrate_kbps == 0 || declared != bitrate_kbps * 1000 {
        return Err(refuse(format!(
            "{named} states {bitrate_kbps} kbit/s and the family '{}' records at {} \
             bit/s; the rate is fixed and a mismatch is refused rather than clocked at \
             a rate nobody stated",
            profile.id, declared
        )));
    }
    Ok(())
}

/// Checks the container's declared RPM against the family's rotation,
/// and answers the cell length the two agree on.
fn check_rotation(named: &str, profile: &'static DriveProfile, rpm: u64) -> Result<u64> {
    let zone = profile
        .density
        .first()
        .ok_or_else(|| refuse(format!("the family '{}' declares no cell rate", profile.id)))?;
    let declared_rpm = profile.rotation.reference_clock * 60 / profile.rotation.cycles_per_rotation;
    if rpm != declared_rpm {
        return Err(refuse(format!(
            "{named} states {rpm} RPM and the family '{}' turns at {declared_rpm}; the \
             rotation is the family's and a mismatch is refused",
            profile.id
        )));
    }
    let cell_cycles =
        profile.rotation.reference_clock * zone.rate_denominator / zone.rate_numerator;
    if cell_cycles == 0 || profile.rotation.cycles_per_rotation % cell_cycles != 0 {
        return Err(refuse(format!(
            "the family '{}' clocks a cell at {cell_cycles} cycles, which does not \
             divide its {} cycle circle; a container of whole cells cannot be laid on \
             it without a remainder this reader will not invent",
            profile.id, profile.rotation.cycles_per_rotation
        )));
    }
    Ok(cell_cycles)
}
