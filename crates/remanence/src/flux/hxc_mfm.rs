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
//! **The rate is the family's, and the container's figure is checked
//! against it rather than clocking anything.** The file states a bit
//! rate and an RPM; the profile states a cell rate and a rotation. Two
//! things real containers do settled how the check reads, and both were
//! learned from HxC's own writer rather than from its format note:
//!
//! - It writes the bit rate it *measured* from the track it converted,
//!   so a 500 kbit/s recording arrives as 501 or 498. A figure within
//!   one part in fifty of the family's rate is the family's rate — the
//!   cells are laid at the family's nominal cell and the container's
//!   figure is declared beside it — and one further off is refused by
//!   name showing both numbers. The band is narrow on purpose: the
//!   nearest rates in use, 250 and 300 kbit/s, sit twenty percent apart.
//! - It writes zero for the RPM, always. Zero says the writer did not
//!   state one, so the family's rotation is taken and the absence is
//!   declared; a non-zero RPM that is not the family's is still refused.
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
    let rate_note = check_rate(named, profile, bitrate_kbps)?;
    let (cell_cycles, rotation_note) = check_rotation(named, profile, rpm)?;

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
        //
        // A real container holds a little more than one revolution — the
        // writer keeps reading past the index it started from, so the
        // tail of a track repeats its head. A cell past the circle is not
        // an angle on it, so those cells are left out and counted rather
        // than laid on a second lap nothing declares.
        let mut pulses = Vec::new();
        let mut cell = 0u64;
        for byte in &bytes[at..end] {
            for shift in (0..8).rev() {
                if cell < cells_per_turn && byte >> shift & 1 == 1 {
                    pulses.push(Pulse::new(cell * cell_cycles, Strength::certain(2)));
                    transitions += 1;
                }
                cell += 1;
            }
        }
        cells_read += cell.min(cells_per_turn);

        if cell > cells_per_turn {
            overrun += 1;
            loss.add(
                "hxc-mfm.track-longer-than-the-circle",
                "the container states more cells for a track than one revolution holds; \
                 the cells past the circle are left out and counted, the circle being \
                 the family's",
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
    let mut evidence = vec![format!(
        "an HxC MFM container of {tracks} track(s) by {sides} side(s), interface \
         mode {interface_type}, stating {bitrate_kbps} kbit/s at {rpm} RPM"
    )];
    evidence.extend(rate_note);
    evidence.extend(rotation_note);
    if overrun > 0 {
        evidence.push(format!(
            "{overrun} track(s) state more cells than one revolution holds, which is the \
             writer reading past the index it started from; the cells past the circle \
             are counted in the declared loss and laid on no second lap"
        ));
    }
    evidence.push(format!(
        "{cells_read} cells laid on the circle as the container states them, \
         {transitions} of them transitions"
    ));
    evidence.push(
        "the container carries no weak region, no density variation and no second \
         observation of a location; every cell is certain, every location is read \
         once, and none of those absences is filled in"
            .to_owned(),
    );
    evidence.push(
        "the cells are the container's own and the transitions beneath them are this \
         reader's restatement of them, declared synthetic rather than presented as \
         recovered timing"
            .to_owned(),
    );
    let report = HxcMfmReport {
        evidence,
        declared_loss: loss.into_entries(),
    };
    Ok((medium, report))
}

/// How far a container's stated bit rate may sit from the family's and
/// still be a measurement of it: one part in fifty, which is wider than
/// any drive's speed tolerance and narrower than the gap to the nearest
/// other rate in use.
const RATE_BAND_DENOMINATOR: u64 = 50;

/// Checks the container's declared bit rate against the family's cell
/// rate, in the family's own terms, answering the evidence to declare
/// where the two agree without being equal.
fn check_rate(
    named: &str,
    profile: &'static DriveProfile,
    bitrate_kbps: u64,
) -> Result<Option<String>> {
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
    // The container states the *data* rate — 250 kbit/s for a
    // double-density disk, 500 for high density — and a family's cell
    // is half a data bit in both FM and MFM, so the family's cell rate
    // is twice the figure the container states. A real container was
    // what showed this: its tracks hold 200,000 cells on a 300 RPM
    // circle and it states 501, which is a megahertz of cells, not half
    // of one.
    let cell_rate = zone.rate_numerator / zone.rate_denominator;
    let family_kbps = cell_rate / 2000;
    let stated = bitrate_kbps * 2000;
    let band = cell_rate / RATE_BAND_DENOMINATOR;
    if bitrate_kbps == 0 || stated.abs_diff(cell_rate) > band {
        return Err(refuse(format!(
            "{named} states {bitrate_kbps} kbit/s and the family '{}' records {family_kbps} \
             kbit/s, a cell rate of {cell_rate} bit/s; the rate is fixed and a mismatch is \
             refused rather than clocked at a rate nobody stated",
            profile.id
        )));
    }
    if stated == cell_rate {
        return Ok(None);
    }
    Ok(Some(format!(
        "the container states {bitrate_kbps} kbit/s, a measured figure within one part \
         in {RATE_BAND_DENOMINATOR} of the family's {family_kbps} kbit/s; the cells are \
         laid at the family's rate and the container's figure is declared rather than \
         clocked"
    )))
}

/// Checks the container's declared RPM against the family's rotation,
/// and answers the cell length the two agree on, with the evidence to
/// declare where the container stated no rotation at all.
fn check_rotation(
    named: &str,
    profile: &'static DriveProfile,
    rpm: u64,
) -> Result<(u64, Option<String>)> {
    let zone = profile
        .density
        .first()
        .ok_or_else(|| refuse(format!("the family '{}' declares no cell rate", profile.id)))?;
    let declared_rpm = profile.rotation.reference_clock * 60 / profile.rotation.cycles_per_rotation;
    // HxC's own writer puts zero here unconditionally, so zero is the
    // writer declining to state a rotation rather than a rotation of
    // zero; the family's is taken and the absence is declared.
    let note = if rpm == 0 {
        Some(format!(
            "the container states no RPM, which is how the HxC writer leaves the field; \
             the family's {declared_rpm} RPM is taken and the absence is declared rather \
             than read as a rotation of zero"
        ))
    } else if rpm != declared_rpm {
        return Err(refuse(format!(
            "{named} states {rpm} RPM and the family '{}' turns at {declared_rpm}; the \
             rotation is the family's and a mismatch is refused",
            profile.id
        )));
    } else {
        None
    };
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
    Ok((cell_cycles, note))
}
