// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The raw rendition of a sector medium: the recording's own sectors,
//! in the recording's own order, as the file every emulator reads.
//!
//! **Raw is bytes and no ecosystem.** A raw image records no article, no
//! drive, no provenance and no coordinates — the reader supplies all of
//! that when it loads one. So this rendition is where the medium's own
//! facts stop travelling, and P29's rule applies in full: what the
//! destination cannot carry is named and counted before anything is
//! produced, rather than being dropped quietly.
//!
//! **The order is the recording's, stated rather than assumed.** The
//! sectors are walked cylinder-major, head-minor, sectors from one —
//! which is what every reader of a raw floppy image expects — and each
//! is placed at the offset the medium's own geometry puts it at. For an
//! ordinary medium that is the whole content in order; where the
//! coordinates cover less than the content holds, the remainder is
//! outside the recording and is declared rather than appended.
//!
//! **The rendition is of committed state.** It reads beneath the
//! session cache, so a caller who forgot the commit point gets the disk
//! as it stands rather than a mix, and the report counts the extents
//! left behind so they are told rather than surprised.

use std::path::Path;

use crate::error::{Error, Result};
use crate::evidence::{DeclaredLoss, LossAccount};
use crate::io::device::{Device, place_new_artifact_streamed};
use crate::model::geometry::{Geometry, GeometryRule, RecordingGeometry, refuse};

/// What a raw rendition carried, and what it did not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawReport {
    /// Where the artifact was written, or `None` for a rendition
    /// computed and not written.
    pub path: Option<String>,
    /// What the artifact occupies: every sector the coordinates
    /// address, and nothing else.
    pub artifact_bytes: u64,
    /// The coordinates the sectors were written in.
    pub geometry: RecordingGeometry,
    pub sectors_written: u64,
    /// Cached extents holding writes the medium has not committed.
    /// They are **not** in the artifact: the rendition is of committed
    /// state, and this is what says how much was left behind.
    pub uncommitted_extents: u64,
    /// What the destination could not carry, in the medium's own terms
    /// (P29).
    pub declared_loss: Vec<DeclaredLoss>,
}

/// Everything the rendition needs from the medium, gathered before a
/// byte moves so that the plan is complete first (P6, P29).
pub(crate) struct RawPlan {
    pub(crate) geometry: RecordingGeometry,
    pub(crate) uncommitted_extents: u64,
    pub(crate) loss: Vec<DeclaredLoss>,
}

impl RawPlan {
    /// The bytes the artifact will hold: every sector the coordinates
    /// address.
    pub(crate) fn artifact_bytes(&self) -> u64 {
        self.geometry.total_bytes()
    }

    pub(crate) fn sectors(&self) -> u64 {
        self.geometry.total_sectors()
    }

    pub(crate) fn report(&self, path: Option<String>) -> RawReport {
        RawReport {
            path,
            artifact_bytes: self.artifact_bytes(),
            geometry: self.geometry,
            sectors_written: self.sectors(),
            uncommitted_extents: self.uncommitted_extents,
            declared_loss: self.loss.clone(),
        }
    }
}

/// What the medium states about itself that a raw artifact cannot hold.
///
/// It is stated as a struct rather than assembled at the call site
/// because each entry is a fact one seam owns: the article is P14's, the
/// provenance is the assurance's, the device is the recording's.
pub(crate) struct RawFacts<'a> {
    pub(crate) article: &'a str,
    pub(crate) article_name: &'a str,
    pub(crate) device_type: Option<&'a str>,
    pub(crate) authored_as: Option<&'a str>,
    pub(crate) recorded_as: Option<&'a str>,
    pub(crate) evidence_lines: usize,
}

/// Plans the rendition: what it will write, and what it cannot carry.
///
/// Every refusal here is about the medium rather than the destination,
/// and each names its own rule: a medium with no coordinates has no
/// order to write its sectors in, and one whose content is shorter than
/// its coordinates address cannot be walked to the end.
pub(crate) fn plan_raw(
    geometry: &Geometry,
    content_bytes: u64,
    uncommitted_extents: u64,
    facts: &RawFacts<'_>,
    named: &str,
) -> Result<RawPlan> {
    let coordinates = geometry.require(named)?;
    let addressed = coordinates.checked_total_bytes().ok_or_else(|| {
        refuse(
            GeometryRule::NotSectorAddressed,
            format!(
                "{named} states coordinates addressing more bytes than any medium \
                 can hold, so there is no artifact to write them into"
            ),
        )
    })?;
    if addressed > content_bytes {
        return Err(Error::unsupported(format!(
            "{named} states coordinates addressing {addressed} bytes and holds \
             {content_bytes}: a raw rendition writes every sector the coordinates \
             address, and this medium cannot be walked to the end of its own"
        )));
    }

    let mut loss = LossAccount::new();
    loss.add(
        "article",
        &format!(
            "the article the medium is — '{}' ({}) — and every passive fact it \
             carries; a raw artifact records bytes and no ecosystem, so a reader \
             of the result declares the article itself",
            facts.article, facts.article_name
        ),
        1,
    );
    match facts.device_type {
        Some(device) => loss.add(
            "device-type",
            &format!(
                "the device the content is recorded by ('{device}'), which a raw \
                 artifact does not state and a load of the result declares"
            ),
            1,
        ),
        None => loss.add(
            "device-type",
            "that no device recorded this content at all, which is a fact about \
             it a raw artifact has nowhere to put",
            1,
        ),
    }
    if let Some(kind) = facts.authored_as {
        loss.add(
            "authored-provenance",
            &format!(
                "that an author created this medium whole as '{kind}', which is \
                 the third fact class and travels no further than this session"
            ),
            1,
        );
    }
    if let Some(layout) = facts.recorded_as {
        loss.add(
            "recorded-layout",
            &format!(
                "the layout its author recorded onto it ('{layout}'); the bytes \
                 that layout wrote are in the artifact, and the fact that a \
                 declaration rather than a disk put them there is not"
            ),
            1,
        );
    }
    if facts.evidence_lines > 0 {
        loss.add(
            "provenance-evidence",
            "the assurance evidence the medium carries — what was established \
             about it, and how — which a raw artifact has no room for",
            facts.evidence_lines as u64,
        );
    }
    if content_bytes > addressed {
        loss.add(
            "content-past-the-coordinates",
            "bytes the medium holds beyond what its coordinates address; the \
             rendition writes the recording, and these are outside it",
            content_bytes - addressed,
        );
    }

    Ok(RawPlan {
        geometry: coordinates,
        uncommitted_extents,
        loss: loss.into_entries(),
    })
}

/// Writes the planned sectors into a new artifact at `path`.
///
/// The sectors are read from `committed` — beneath the session cache, so
/// what lands is the medium's committed state — in the recording's own
/// order, and streamed into the destination as they are read: a rendition
/// of a large medium costs one sector of memory rather than the whole
/// disk (P27).
pub(crate) fn write_raw_artifact(
    path: &Path,
    plan: &RawPlan,
    committed: &mut dyn Device,
) -> Result<()> {
    let coordinates = plan.geometry;
    let sector_bytes = coordinates.sector_bytes as usize;
    let mut sector = vec![0u8; sector_bytes];
    place_new_artifact_streamed(path, plan.artifact_bytes(), |sink| {
        let mut at = 0u64;
        for cylinder in 0..coordinates.cylinders {
            for head in 0..coordinates.heads {
                // Sectors number from one, which is the recording's
                // convention rather than this library's.
                for number in 1..=coordinates.sectors_per_track {
                    let offset = coordinates.offset_of(cylinder, head, number)?;
                    committed.read_at(offset, &mut sector)?;
                    sink(at, &sector)?;
                    at += sector_bytes as u64;
                }
            }
        }
        Ok(())
    })
}
