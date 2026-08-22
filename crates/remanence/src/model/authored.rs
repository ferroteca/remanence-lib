// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! Authored media: **the third fact class**, and the blanks it creates
//! whole.
//!
//! Evidence is discovered onto media and declarations are configured onto
//! machines; **authorship creates media whole**, and nothing crosses.
//! [`Session::new_media`](crate::Session::new_media) is where the third
//! class enters: there is no artifact yet, nothing is read, and the facts
//! the author states at creation *are* the medium's original facts —
//! carried as provenance on its
//! [`assurance`](crate::Medium::assurance) and, where the kind states
//! coordinates, as its [`geometry`](crate::Medium::geometry), whose one
//! reading is the author's own.
//!
//! **An authored blank assumes no device until something is recorded
//! onto it.** [`device_type()`](crate::Medium::device_type) answers
//! `None` — the same honest absence an archive answers, for the same
//! reason: nothing recorded it — so no drive takes one and
//! [`insert`](crate::DeviceView::insert) refuses by name.
//!
//! **The arc from authored to recorded is
//! [`record_as`](crate::PartitionView::record_as)** (F82): recording a
//! published DOS layout onto a blank article makes the medium a
//! recorded one. It binds the drive the layout is recorded for, gives
//! the medium the layout's coordinates, and lays down the boot record
//! the FAT seam then reads for itself. What the arc still does not do is
//! record a *partition table* onto anything — a partition editor
//! consuming coordinates into MBR end tuples stays reserved.
//!
//! **The kinds are an enumerated claim** (P3), like every other creation
//! grammar here: the **blank article kinds**, each naming one entry of
//! the article catalog and creating that manufactured substrate with
//! nothing recorded on it, and [`NewMedia::ChsDisk`], whose article is
//! authored rather than manufactured and whose content is reached in the
//! coordinates the author stated.
//!
//! **A blank is session-backed** until an explicit encode gives it an
//! artifact. Its content lives in the session's own bounded working set
//! (P27) — resident within the declared bound, spilled to private
//! session storage past it — over a sparse blank backing that holds only
//! the extents anything was ever written to. The commit point is the
//! ordinary one (P2): writes buffer until
//! [`commit`](crate::Medium::commit) makes them the medium's own state,
//! and [`rollback`](crate::Medium::rollback) discards them. There is no
//! journal, because there is no artifact for an interruption to leave
//! half-written (P9 governs the file a commit changes, and an authored
//! medium changes none).

use std::collections::BTreeMap;
use std::fs::File;

use crate::error::{Error, Result};
use crate::io::cache::{EXTENT, SessionCache, session_storage_file};
use crate::io::device::{AccessMode, Claim, Device, read_exact_at, write_all_at};
use crate::model::assurance::Assurance;
use crate::model::device_type::DeviceType;
use crate::model::geometry::{Geometry, RecordingGeometry};
use crate::model::media_profile::{
    AUTHORED, FLEXIBLE_3_5_HD, FLEXIBLE_5_25_HARD_10, FLEXIBLE_5_25_HD, FLEXIBLE_5_25_SOFT,
    MediaProfile,
};
use crate::model::recording::Recording;
use crate::model::session::{
    Identification, Layer, LayerKind, LayerLayout, PhysicalMediaLayout, SizeInformation,
};

/// One authored kind, declared at [`Session::new_media`](crate::Session::new_media).
///
/// **A declaration names a concrete catalog entry, never a
/// classification** (P3) — the same rule
/// [`Format`](crate::Format) follows at the load, and
/// [`PartitionType`](crate::PartitionType) at a partition. The set below
/// is exactly what this release authors, and a kind it does not claim
/// fails to compile rather than being spelled and refused at run time.
///
/// **The blank article kinds name their article and carry nothing
/// else.** A blank manufactured disk *is* its article — a coating, a
/// form factor, the holes punched in it — and nothing is recorded on it,
/// so it states no coordinates and bears no content until the
/// authored-to-recorded arc records some. [`NewMedia::ChsDisk`] is the
/// other half: no manufactured article stands behind it, so its article
/// is the authored one, and what the author states is the recording's
/// own coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NewMedia {
    /// A blank 5.25-inch soft-sectored flexible disk — the article a
    /// 1541 and an H-37 are both served, with nothing recorded on it.
    Flexible525Soft,
    /// A blank 5.25-inch ten-sector hard-sectored flexible disk — the
    /// article an H-17 is served, whose ten holes divide the revolution
    /// whether or not anything is recorded between them.
    Flexible525HardTen,
    /// A blank 5.25-inch high-density flexible disk — the article a
    /// PC's 1.2 MB drive is served, with nothing recorded on it. The
    /// same jacket as the soft-sectored disk above and a different
    /// article: twice the coercivity, twice the track density.
    Flexible525Hd,
    /// A blank 3.5-inch high-density flexible disk — the article a
    /// PC's 1.44 MB drive is served, with nothing recorded on it.
    Flexible35Hd,
    /// A blank disk addressed in coordinates the author states: so many
    /// cylinders of so many heads, at so many sectors of so many bytes.
    ///
    /// The coordinates are the delivered [`RecordingGeometry`]'s own, and
    /// they arrive here as authorship rather than as evidence — which is
    /// the whole distinction the fact classes draw. Every part must be
    /// stated: a geometry is whole or it addresses nothing.
    ChsDisk { geometry: RecordingGeometry },
}

/// What one authored kind creates: its identity, the article it is, and
/// whether its declaration carries coordinates.
///
/// It is the enumerated claim read the other way round — the same
/// declarations the [`NewMedia`] variants spell, in a shape the text
/// boundaries (C, Python) can enumerate and refuse against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NewMediaClaim {
    id: &'static str,
    name: &'static str,
    article: &'static str,
    geometry: bool,
}

impl NewMediaClaim {
    /// The stable cross-language spelling.
    pub const fn id(&self) -> &'static str {
        self.id
    }

    /// The kind's name, fit to show a user.
    pub const fn name(&self) -> &'static str {
        self.name
    }

    /// The article a medium of this kind is, by the article catalog's own
    /// stable spelling — the manufactured substrate for a blank article
    /// kind, and the authored article where no manufactured one stands
    /// behind it.
    pub const fn article(&self) -> &'static str {
        self.article
    }

    /// Whether a declaration of this kind carries the recording's
    /// coordinates — true for the CHS disk alone, which is the kind whose
    /// facts *are* coordinates.
    pub const fn takes_geometry(&self) -> bool {
        self.geometry
    }
}

/// The stable spelling of the kind that states coordinates, named once:
/// several refusals point a caller at it.
const CHS_DISK: &str = "chs-disk";

/// Every kind [`Session::new_media`](crate::Session::new_media) authors,
/// and what each creates. The catalog is the claim: a kind absent from it
/// is refused by name.
static CLAIMED: [NewMediaClaim; 5] = [
    NewMediaClaim {
        id: "flexible-5.25-soft",
        name: "blank 5.25-inch soft-sectored flexible disk",
        article: "flexible-5.25-soft",
        geometry: false,
    },
    NewMediaClaim {
        id: "flexible-5.25-hard-10",
        name: "blank 5.25-inch ten-sector hard-sectored flexible disk",
        article: "flexible-5.25-hard-10",
        geometry: false,
    },
    NewMediaClaim {
        id: "flexible-5.25-hd",
        name: "blank 5.25-inch high-density flexible disk",
        article: "flexible-5.25-hd",
        geometry: false,
    },
    NewMediaClaim {
        id: "flexible-3.5-hd",
        name: "blank 3.5-inch high-density flexible disk",
        article: "flexible-3.5-hd",
        geometry: false,
    },
    NewMediaClaim {
        id: CHS_DISK,
        name: "blank disk in authored cylinder, head and sector coordinates",
        article: "authored",
        geometry: true,
    },
];

impl NewMedia {
    /// Every kind an authored creation may declare, with what each
    /// creates.
    pub fn claimed() -> &'static [NewMediaClaim] {
        &CLAIMED
    }

    /// The stable cross-language spelling, which is what the C and Python
    /// surfaces carry and what a refusal quotes back.
    ///
    /// A blank article kind is spelled by its **article**, because that is
    /// the whole of what it declares.
    pub const fn id(self) -> &'static str {
        match self {
            Self::Flexible525Soft => "flexible-5.25-soft",
            Self::Flexible525HardTen => "flexible-5.25-hard-10",
            Self::Flexible525Hd => "flexible-5.25-hd",
            Self::Flexible35Hd => "flexible-3.5-hd",
            Self::ChsDisk { .. } => CHS_DISK,
        }
    }

    /// The kind's name, fit to show a user.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Flexible525Soft => "blank 5.25-inch soft-sectored flexible disk",
            Self::Flexible525HardTen => "blank 5.25-inch ten-sector hard-sectored flexible disk",
            Self::Flexible525Hd => "blank 5.25-inch high-density flexible disk",
            Self::Flexible35Hd => "blank 3.5-inch high-density flexible disk",
            Self::ChsDisk { .. } => "blank disk in authored cylinder, head and sector coordinates",
        }
    }

    /// What this kind creates — its own entry in the catalog above.
    pub fn claim(self) -> &'static NewMediaClaim {
        CLAIMED
            .iter()
            .find(|claim| claim.id == self.id())
            .expect("every variant has a catalog entry")
    }

    /// The article a medium of this kind is (P14), by the article
    /// catalog's stable spelling.
    pub fn article(self) -> &'static str {
        self.article_profile().id
    }

    /// The coordinates this declaration states, where the kind states
    /// any.
    pub fn geometry(self) -> Option<RecordingGeometry> {
        match self {
            Self::ChsDisk { geometry } => Some(geometry),
            Self::Flexible525Soft
            | Self::Flexible525HardTen
            | Self::Flexible525Hd
            | Self::Flexible35Hd => None,
        }
    }

    pub(crate) fn article_profile(self) -> &'static MediaProfile {
        match self {
            Self::Flexible525Soft => &FLEXIBLE_5_25_SOFT,
            Self::Flexible525HardTen => &FLEXIBLE_5_25_HARD_10,
            Self::Flexible525Hd => &FLEXIBLE_5_25_HD,
            Self::Flexible35Hd => &FLEXIBLE_3_5_HD,
            Self::ChsDisk { .. } => &AUTHORED,
        }
    }

    /// Builds a declaration from the stable spellings, for the C and
    /// Python surfaces where one arrives as text (P5).
    ///
    /// Each half is refused by name on its own terms: a kind this release
    /// does not author, coordinates where the kind states none, and no
    /// coordinates where the kind is nothing but coordinates.
    pub fn declared(kind: &str, geometry: Option<RecordingGeometry>) -> Result<Self> {
        let claim = CLAIMED
            .iter()
            .find(|claim| claim.id == kind)
            .ok_or_else(|| {
                let claimed: Vec<&str> = CLAIMED.iter().map(|claim| claim.id).collect();
                Error::unsupported(format!(
                    "'{kind}' names no medium this release authors; the kinds it \
                 authors are {}",
                    claimed.join(", ")
                ))
            })?;
        if geometry.is_some() && !claim.geometry {
            return Err(Error::unsupported(format!(
                "the {} is the article itself with nothing recorded on it, so \
                 it states no cylinder, head or sector: coordinates are \
                 authored with '{}'",
                claim.name, CHS_DISK
            )));
        }
        Ok(match claim.id {
            "flexible-5.25-soft" => Self::Flexible525Soft,
            "flexible-5.25-hard-10" => Self::Flexible525HardTen,
            "flexible-5.25-hd" => Self::Flexible525Hd,
            "flexible-3.5-hd" => Self::Flexible35Hd,
            CHS_DISK => Self::ChsDisk {
                geometry: geometry.ok_or_else(|| {
                    Error::unsupported(format!(
                        "the {} is its coordinates, so its declaration carries \
                         them: cylinders, heads, sectors per track and the \
                         sector size, every part stated",
                        claim.name
                    ))
                })?,
            },
            other => unreachable!("'{other}' is not a claimed authored kind"),
        })
    }

    /// Checks that what the author stated can be a medium at all, before
    /// anything is created.
    ///
    /// A geometry with a zero in it addresses nothing, and one whose
    /// product no medium could hold is refused here rather than at the
    /// first sector: an authored blank's facts are checked when they are
    /// stated, which is the only moment authorship offers.
    fn check(self) -> Result<()> {
        let Some(geometry) = self.geometry() else {
            return Ok(());
        };
        let parts = [
            ("cylinders", u64::from(geometry.cylinders)),
            ("heads", u64::from(geometry.heads)),
            ("sectors per track", u64::from(geometry.sectors_per_track)),
            ("bytes per sector", geometry.sector_bytes),
        ];
        for (part, stated) in parts {
            if stated == 0 {
                return Err(Error::unsupported(format!(
                    "an authored disk of {stated} {part} addresses nothing: a \
                     geometry is whole or it is nothing, so every part of it is \
                     stated and every part is more than zero"
                )));
            }
        }
        geometry.checked_total_bytes().map(drop).ok_or_else(|| {
            Error::unsupported(format!(
                "the authored coordinates {geometry} address more bytes than \
                 any medium can hold"
            ))
        })
    }
}

impl std::fmt::Display for NewMedia {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.id())
    }
}

/// A medium the author created whole: its article, the facts it was
/// stated with, and the session-backed content its kind gives it.
#[derive(Debug)]
pub(crate) struct AuthoredMedium {
    kind: NewMedia,
    article: &'static MediaProfile,
    /// The author's own coordinates, where the kind states any — a
    /// [`Geometry`] whose one reading is authorship, settled by
    /// construction because the author stated every part of it.
    geometry: Geometry,
    assurance: Assurance,
    /// The content the kind gives the medium, absent for a blank article
    /// kind until something is recorded onto it — a manufactured blank
    /// has an article and no recording, and a recording is what a space
    /// would be a position within.
    space: Option<AuthoredSpace>,
    /// The layout recorded onto this blank article, where the arc has
    /// run. It is what makes the medium a recorded one: the coordinates
    /// above are the layout's from then on, and the device is the drive
    /// it is recorded for.
    recorded: Option<Recording>,
    /// The session cache bound the medium was created under (P27), which
    /// is what bounds the content the arc gives it.
    cache_bytes: u64,
}

impl AuthoredMedium {
    /// Creates the medium the author declared, whole.
    ///
    /// Nothing is read and nothing is probed: there is no artifact yet,
    /// so the declaration is checked against itself and the facts it
    /// states become the medium's own.
    pub(crate) fn create(kind: NewMedia, cache_bytes: u64) -> Result<Self> {
        kind.check()?;
        let article = kind.article_profile();
        let space = kind.geometry().map(|geometry| {
            AuthoredSpace::new(
                geometry
                    .checked_total_bytes()
                    .expect("the declaration was checked"),
                cache_bytes,
            )
        });
        let mut assurance = Assurance::verified(
            space.as_ref().map_or(0, AuthoredSpace::size),
            AccessMode::ReadWrite,
            Claim::Authored,
        );
        assurance.evidence = evidence(kind, article, space.as_ref());
        Ok(Self {
            kind,
            article,
            geometry: match kind.geometry() {
                Some(coordinates) => Geometry::authored(coordinates),
                None => Geometry::unstated(),
            },
            assurance,
            space,
            recorded: None,
            cache_bytes,
        })
    }

    /// Records a published layout onto this blank article — the
    /// authored-to-recorded arc (F82).
    ///
    /// **It records onto a blank article, once.** A medium the author
    /// stated coordinates for is not a manufactured article and has its
    /// own facts already; one that has been recorded onto is a recorded
    /// medium, and recording over it would discard what the author put
    /// there. Both refuse by name.
    ///
    /// **The layout declares which article it fits**, and the check is
    /// the catalog's: the 1.44 MB layout is not laid onto a 5.25-inch
    /// disk because a caller asked for it.
    ///
    /// This is an act of authorship rather than a buffered write, which
    /// is why nothing here waits for a commit: the medium *becomes* a
    /// recorded one in this call, exactly as `new_media` makes one whole
    /// in its own. The ordinary commit point (P2) governs everything
    /// written afterwards.
    pub(crate) fn record_as(&mut self, layout: Recording) -> Result<()> {
        if let Some(already) = self.recorded {
            return Err(Error::unsupported(format!(
                "{} already carries the {} layout, recorded onto it by its \
                 author; the arc records onto a blank article once, and a \
                 medium that has been recorded onto is read through the \
                 namespace it now bears rather than recorded over",
                self.named(),
                already.id()
            )));
        }
        if self.kind.geometry().is_some() {
            return Err(Error::unsupported(format!(
                "{} states the author's own coordinates, and a layout is \
                 recorded onto a manufactured article rather than onto \
                 coordinates somebody already stated: the arc takes a blank \
                 article kind, which is the article and nothing else",
                self.named()
            )));
        }
        if layout.article_profile().id != self.article.id {
            return Err(Error::unsupported(format!(
                "the {} is recorded onto the article '{}' and this medium is a \
                 {} ('{}'): a layout declares the article it fits, and this \
                 release does not lay it onto another",
                layout.name(),
                layout.article_profile().id,
                self.article.name,
                self.article.id
            )));
        }

        let mut space = AuthoredSpace::new(
            layout
                .geometry()
                .checked_total_bytes()
                .expect("a claimed layout's coordinates are whole"),
            self.cache_bytes,
        );
        layout.lay_down(&mut space)?;
        // The arc is one act: what it lays down is the medium's own
        // state when the call returns, not a buffered write awaiting a
        // commit that might never come.
        space.commit()?;

        self.assurance.evidence.push(format!(
            "the author recorded a layout onto the blank at record_as: {}",
            layout.describe()
        ));
        self.assurance.evidence.push(format!(
            "the medium is a recording from here on: its coordinates are the \
             layout's, the device that records it is {} ({}), and the boot \
             record just laid down is what the filesystem seam reads for \
             itself",
            layout.device_type().name(),
            layout.device_type().id()
        ));
        self.geometry = Geometry::recorded(layout.geometry(), layout.id());
        self.space = Some(space);
        self.recorded = Some(layout);
        Ok(())
    }

    /// The layout recorded onto this medium, where the arc has run.
    pub(crate) fn recorded_as(&self) -> Option<Recording> {
        self.recorded
    }

    /// The device this medium's content is recorded by — the drive the
    /// layout is recorded for, and `None` while nothing is recorded on
    /// it at all.
    pub(crate) fn device_type(&self) -> Option<DeviceType> {
        self.recorded.map(Recording::device_type)
    }

    /// The kind the author declared.
    pub(crate) fn kind(&self) -> NewMedia {
        self.kind
    }

    /// How a refusal names an authored medium. There is no path to quote
    /// — nothing was opened — so it names what the author made.
    pub(crate) fn named(&self) -> String {
        format!("this authored medium (a {})", self.kind.name())
    }

    pub(crate) fn media(&self) -> &'static MediaProfile {
        self.article
    }

    /// An authored blank is writable: the author made it, and there is no
    /// evidence beneath it to narrow what they may do with it.
    pub(crate) fn mode(&self) -> AccessMode {
        self.assurance.access
    }

    pub(crate) fn assurance(&self) -> &Assurance {
        &self.assurance
    }

    /// The author's own coordinates, or the ordinary settling of nothing
    /// where the kind states none.
    pub(crate) fn geometry(&self) -> Geometry {
        self.geometry.clone()
    }

    /// Whether this kind's content is addressed by the recording's own
    /// cylinder, head and sector — true exactly where the author stated
    /// coordinates.
    pub(crate) fn is_sector_addressed(&self) -> bool {
        self.kind.geometry().is_some() || self.recorded.is_some()
    }

    pub(crate) fn is_modified(&self) -> bool {
        self.space.as_ref().is_some_and(AuthoredSpace::is_modified)
    }

    /// How many cached extents hold uncommitted writes.
    pub(crate) fn uncommitted_extents(&self) -> u64 {
        self.space
            .as_ref()
            .map_or(0, AuthoredSpace::uncommitted_extents)
    }

    /// The layered identification of a medium no artifact holds: the
    /// article, and nothing beneath it — an authored blank has no image
    /// format because it has no image.
    pub(crate) fn identify(&self) -> Identification {
        Identification {
            layers: vec![Layer {
                kind: LayerKind::PhysicalMedia,
                id: self.article.id.to_owned(),
                name: self.article.name.to_owned(),
                confidence: 100,
                known: true,
                size: SizeInformation {
                    current_bytes: self.space.as_ref().map(AuthoredSpace::size),
                    expected_bytes: None,
                },
                layout: LayerLayout::PhysicalMedia(PhysicalMediaLayout::Unknown),
            }],
            modified: self.is_modified(),
            evidence: self.assurance.evidence.clone(),
        }
    }

    /// The content this medium bears, or the refusal naming the blank
    /// that bears none.
    pub(crate) fn space(&self, verb: &str) -> Result<&AuthoredSpace> {
        self.space.as_ref().ok_or_else(|| self.no_content(verb))
    }

    pub(crate) fn space_mut(&mut self, verb: &str) -> Result<&mut AuthoredSpace> {
        match &mut self.space {
            Some(space) => Ok(space),
            None => Err(no_content(self.kind, self.article, verb)),
        }
    }

    fn no_content(&self, verb: &str) -> Error {
        no_content(self.kind, self.article, verb)
    }

    /// The refusal an authored medium answers the verbs of an artifact
    /// and of a namespace with — both at once, because it has neither for
    /// the same reason.
    pub(crate) fn no_image(&self, verb: &str) -> Error {
        Error::unsupported(format!(
            "'{verb}' reads a medium the way an image format presents it — the \
             container it is, the layout recorded on it, the namespace above \
             that — and {} has none of them: it was created whole by the \
             author and is session-backed until an explicit encode gives it \
             an artifact",
            self.named()
        ))
    }
}

/// The refusal a blank article kind answers a content verb with.
fn no_content(kind: NewMedia, article: &'static MediaProfile, verb: &str) -> Error {
    Error::unsupported(format!(
        "'{verb}' addresses content and this authored medium is a {} — the \
         {} itself, with nothing recorded on it, which is the whole of what \
         the kind states. Content arrives either way: an authored blank whose \
         content is addressed states its coordinates at creation ('{}'), and a \
         blank article takes a published layout through `record_as` on its \
         direct partition",
        kind.name(),
        article.name,
        CHS_DISK
    ))
}

/// The author's facts, stated as the provenance the medium carries from
/// creation (P4): what was made, what article it is, what the author
/// said about it, and what authorship does *not* assert.
fn evidence(
    kind: NewMedia,
    article: &'static MediaProfile,
    space: Option<&AuthoredSpace>,
) -> Vec<String> {
    let mut evidence = vec![
        format!(
            "created whole by the author at new_media: a {}, which is the \
             article '{}' ({})",
            kind.name(),
            article.id,
            article.name
        ),
        "authorship is the third fact class beside discovery and \
         declaration: nothing here was read off an artifact, and these are \
         the medium's original facts"
            .to_owned(),
    ];
    match kind.geometry() {
        Some(geometry) => evidence.push(format!(
            "the author states the recording's coordinates: {geometry} — the \
             medium's geometry, whose one reading is authorship",
        )),
        None => evidence.push(format!(
            "nothing is recorded on it, so it states no coordinates: {}",
            article.provenance
        )),
    }
    if let Some(space) = space {
        evidence.push(format!(
            "session-backed: {} bytes of blank content held in the session's \
             own bounded working set until an explicit encode gives it an \
             artifact",
            space.size()
        ));
    }
    evidence.push(
        "no device is assumed — device_type() answers none — until a layout is \
         recorded onto it, which is what binds one"
            .to_owned(),
    );
    evidence
}

/// The content an authored blank bears: the commit-point buffer (P2)
/// over the sparse blank backing, both bounded by the session's declared
/// cache budget (P27).
#[derive(Debug)]
pub(crate) struct AuthoredSpace {
    backing: BlankBacking,
    cache: SessionCache,
}

impl AuthoredSpace {
    fn new(length: u64, cache_bytes: u64) -> Self {
        Self {
            backing: BlankBacking::of(length),
            cache: SessionCache::with_bytes_offloading(cache_bytes),
        }
    }

    /// How many bytes the author's coordinates address — which is the
    /// whole of this medium's content.
    pub(crate) fn size(&self) -> u64 {
        self.backing.length
    }

    pub(crate) fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<()> {
        self.backing.within(offset, buf.len() as u64, "read")?;
        self.cache.read_at(&mut self.backing, offset, buf)
    }

    /// Buffered until [`AuthoredSpace::commit`] like every other write
    /// (P2).
    ///
    /// **The bound is checked here rather than at the commit**, because
    /// here is the only place that knows it: an image format's adapter
    /// answers for the disk it presents, and an authored medium has no
    /// adapter beneath it — what the author's coordinates address is the
    /// whole of what exists, and a write past it would otherwise buffer
    /// and be dropped when the commit clamped it.
    pub(crate) fn write_at(&mut self, offset: u64, data: &[u8]) -> Result<()> {
        self.backing.within(offset, data.len() as u64, "write")?;
        self.cache.write_at(&mut self.backing, offset, data)
    }

    pub(crate) fn is_modified(&self) -> bool {
        self.cache.modified()
    }

    /// How many cached extents hold uncommitted writes.
    pub(crate) fn uncommitted_extents(&self) -> u64 {
        self.cache.dirty_extents()
    }

    /// The sparse backing beneath the cache — the plane a commit writes
    /// into, and so this medium's committed state.
    pub(crate) fn committed_device(&mut self) -> &mut dyn Device {
        &mut self.backing
    }

    /// The commit point (P2), on a medium with no artifact: the buffered
    /// writes become the medium's own state and stop being rollback-able.
    ///
    /// There is no recovery journal and no durability boundary to cross,
    /// because nothing outside the session changes — P9 makes the change
    /// to a *file* reconcilable, and an authored blank changes none until
    /// an explicit encode gives it one.
    pub(crate) fn commit(&mut self) -> Result<()> {
        if !self.cache.modified() {
            return Ok(());
        }
        self.cache.join_offloads();
        self.cache.write_through(&mut self.backing)?;
        self.cache.mark_committed();
        Ok(())
    }

    /// Discards everything buffered since the medium was created or last
    /// committed; the committed content is untouched.
    pub(crate) fn rollback(&mut self) {
        self.cache.discard_dirty();
    }
}

impl Device for AuthoredSpace {
    fn len(&self) -> u64 {
        self.size()
    }

    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<()> {
        Self::read_at(self, offset, buf)
    }

    fn write_at(&mut self, offset: u64, data: &[u8]) -> Result<()> {
        Self::write_at(self, offset, data)
    }

    /// Nothing to flush: the commit point is
    /// [`AuthoredSpace::commit`], and there is no artifact underneath for
    /// a flush to reach.
    fn flush(&mut self) -> Result<()> {
        Ok(())
    }
}

/// The blank a medium is authored over: every byte reads as zero until
/// something is committed to it, and what is committed lives in private
/// session storage.
///
/// It is **sparse by construction**, which is what makes a large authored
/// disk cost what was written to it rather than what it addresses (P27):
/// an extent nothing has been committed to has no slot and reads as
/// zeros, and one that does keeps its slot for the session. The storage
/// is the same private transient state the cache spills to — unlinked at
/// birth (POSIX) or delete-on-close (Windows), no user-owned path, no
/// cleanup verb — so an authored medium cannot outlive its session, which
/// is what "session-backed" means.
#[derive(Debug)]
struct BlankBacking {
    length: u64,
    storage: Option<File>,
    /// Extent offset in the authored content -> slot index in storage.
    slots: BTreeMap<u64, u64>,
    next_slot: u64,
}

impl BlankBacking {
    fn of(length: u64) -> Self {
        Self {
            length,
            storage: None,
            slots: BTreeMap::new(),
            next_slot: 0,
        }
    }

    fn within(&self, offset: u64, length: u64, act: &str) -> Result<()> {
        offset
            .checked_add(length)
            .filter(|end| *end <= self.length)
            .map(drop)
            .ok_or_else(|| {
                Error::io(format!(
                    "this authored medium holds {} bytes and the {act} at \
                     {offset} reaches past it",
                    self.length
                ))
            })
    }

    /// One whole extent of the authored content: what was committed to it,
    /// or the zeros a blank reads as.
    fn extent(&self, extent_offset: u64, into: &mut [u8]) -> Result<()> {
        let Some(slot) = self.slots.get(&extent_offset) else {
            into.fill(0);
            return Ok(());
        };
        let storage = self
            .storage
            .as_ref()
            .expect("a committed extent has storage behind it");
        read_exact_at(storage, slot * EXTENT, into).map_err(|error| {
            Error::io(format!(
                "cannot read the authored medium's session storage: {error}"
            ))
        })
    }

    fn slot_for(&mut self, extent_offset: u64) -> Result<u64> {
        if let Some(slot) = self.slots.get(&extent_offset) {
            return Ok(*slot);
        }
        if self.storage.is_none() {
            self.storage = Some(session_storage_file()?);
        }
        let slot = self.next_slot;
        self.next_slot += 1;
        self.slots.insert(extent_offset, slot);
        Ok(slot)
    }
}

impl Device for BlankBacking {
    fn len(&self) -> u64 {
        self.length
    }

    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<()> {
        self.within(offset, buf.len() as u64, "read")?;
        let end = offset + buf.len() as u64;
        let mut whole = vec![0u8; EXTENT as usize];
        let mut extent_offset = offset / EXTENT * EXTENT;
        while extent_offset < end {
            self.extent(extent_offset, &mut whole)?;
            let from = extent_offset.max(offset);
            let to = (extent_offset + EXTENT).min(end);
            buf[(from - offset) as usize..(to - offset) as usize].copy_from_slice(
                &whole[(from - extent_offset) as usize..(to - extent_offset) as usize],
            );
            extent_offset += EXTENT;
        }
        Ok(())
    }

    fn write_at(&mut self, offset: u64, data: &[u8]) -> Result<()> {
        self.within(offset, data.len() as u64, "write")?;
        let end = offset + data.len() as u64;
        let mut whole = vec![0u8; EXTENT as usize];
        let mut extent_offset = offset / EXTENT * EXTENT;
        while extent_offset < end {
            // Read-modify-write, so a partial write keeps whatever was
            // committed around it rather than blanking the extent.
            self.extent(extent_offset, &mut whole)?;
            let from = extent_offset.max(offset);
            let to = (extent_offset + EXTENT).min(end);
            whole[(from - extent_offset) as usize..(to - extent_offset) as usize]
                .copy_from_slice(&data[(from - offset) as usize..(to - offset) as usize]);
            let slot = self.slot_for(extent_offset)?;
            let storage = self.storage.as_ref().expect("just ensured");
            write_all_at(storage, slot * EXTENT, &whole).map_err(|error| {
                Error::io(format!(
                    "cannot write the authored medium's session storage: {error}"
                ))
            })?;
            extent_offset += EXTENT;
        }
        Ok(())
    }

    fn flush(&mut self) -> Result<()> {
        // Private transient state: there is no artifact to make durable,
        // and the storage cannot outlive the session that holds it.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Resolves the article a kind creates, for the assertion that the
    /// two catalogs agree.
    fn claimed_article(claim: &NewMediaClaim) -> &'static MediaProfile {
        crate::model::media_profile::by_id(claim.article())
            .expect("an authored kind names an enrolled article")
    }

    fn chs() -> RecordingGeometry {
        RecordingGeometry {
            cylinders: 40,
            heads: 2,
            sectors_per_track: 9,
            sector_bytes: 512,
        }
    }

    #[test]
    fn every_claimed_kind_round_trips_through_its_spelling() {
        for claim in NewMedia::claimed() {
            let geometry = claim.takes_geometry().then(chs);
            let kind = NewMedia::declared(claim.id(), geometry).expect("claimed");
            assert_eq!(kind.id(), claim.id());
            assert_eq!(kind.name(), claim.name());
            assert_eq!(kind.article(), claim.article());
            assert_eq!(kind.geometry(), geometry);
            assert_eq!(kind.claim(), claim);
            assert_eq!(
                claimed_article(claim).id,
                claim.article(),
                "an authored kind's article is one the article catalog enrolls"
            );
        }
    }

    #[test]
    fn a_classification_is_not_a_declaration() {
        // P3: the set is enumerated, so a word naming a kind of blank
        // rather than one catalog entry is refused naming what is claimed.
        let error = NewMedia::declared("blank", None).expect_err("refused");
        let message = error.to_string();
        assert!(message.contains("blank"), "names what was asked: {message}");
        assert!(
            message.contains("chs-disk") && message.contains("flexible-5.25-soft"),
            "names what is authored: {message}"
        );
    }

    #[test]
    fn coordinates_are_carried_where_and_only_where_the_kind_states_them() {
        assert!(
            NewMedia::declared("chs-disk", None).is_err(),
            "the CHS disk is its coordinates"
        );
        let error =
            NewMedia::declared("flexible-5.25-soft", Some(chs())).expect_err("a blank states none");
        assert!(
            error.to_string().contains("nothing recorded on it"),
            "names why a blank article states none: {error}"
        );
    }

    #[test]
    fn a_geometry_with_a_hole_in_it_is_refused_when_it_is_stated() {
        // Authorship offers one moment to check the author's facts, and
        // this is it: a zero anywhere addresses nothing.
        for geometry in [
            RecordingGeometry {
                cylinders: 0,
                ..chs()
            },
            RecordingGeometry { heads: 0, ..chs() },
            RecordingGeometry {
                sectors_per_track: 0,
                ..chs()
            },
            RecordingGeometry {
                sector_bytes: 0,
                ..chs()
            },
        ] {
            let error = AuthoredMedium::create(NewMedia::ChsDisk { geometry }, 1 << 20)
                .expect_err("addresses nothing");
            assert!(
                error.to_string().contains("whole or it is nothing"),
                "{error}"
            );
        }
        let error = AuthoredMedium::create(
            NewMedia::ChsDisk {
                geometry: RecordingGeometry {
                    cylinders: u32::MAX,
                    heads: u32::MAX,
                    sectors_per_track: u32::MAX,
                    sector_bytes: u64::MAX,
                },
            },
            1 << 20,
        )
        .expect_err("no medium holds that");
        assert!(error.to_string().contains("more bytes than"), "{error}");
    }

    #[test]
    fn an_authored_blank_carries_the_authors_facts_as_its_own() {
        let authored = AuthoredMedium::create(NewMedia::ChsDisk { geometry: chs() }, 1 << 20)
            .expect("created");
        assert_eq!(authored.media().id, "authored");
        assert_eq!(authored.mode(), AccessMode::ReadWrite);
        assert_eq!(authored.assurance().claim, Claim::Authored);
        assert_eq!(
            authored.geometry().determined(),
            Some(chs()),
            "the author stated every part, so nothing is left unsettled"
        );
        assert!(authored.is_sector_addressed());
        assert_eq!(authored.space("size").expect("content").size(), 368_640);
        assert!(
            authored
                .assurance()
                .evidence
                .iter()
                .any(|line| line.contains("third fact class")),
            "{:?}",
            authored.assurance().evidence
        );
        assert!(
            authored
                .assurance()
                .evidence
                .iter()
                .any(|line| line.contains("no device is assumed")),
            "{:?}",
            authored.assurance().evidence
        );
    }

    #[test]
    fn a_blank_article_is_the_article_and_bears_no_content() {
        let mut authored =
            AuthoredMedium::create(NewMedia::Flexible525HardTen, 1 << 20).expect("created");
        assert_eq!(authored.media().id, "flexible-5.25-hard-10");
        assert!(!authored.is_sector_addressed());
        assert_eq!(authored.geometry().determined(), None);
        let error = authored.space_mut("write_sector").expect_err("bears none");
        let message = error.to_string();
        assert!(
            message.contains("nothing recorded on it"),
            "names what it is: {message}"
        );
        assert!(
            message.contains("chs-disk"),
            "names where coordinates are authored: {message}"
        );
    }

    #[test]
    fn the_blank_reads_as_zeros_and_keeps_only_what_was_written() {
        let mut space = AuthoredSpace::new(4 * EXTENT, 1 << 20);
        let mut buf = [0xffu8; 32];
        space.read_at(0, &mut buf).expect("reads");
        assert_eq!(buf, [0u8; 32], "a blank reads as zeros");

        space
            .write_at(3 * EXTENT + 10, b"authored")
            .expect("writes");
        assert!(space.is_modified());
        space.commit().expect("commits");
        assert!(!space.is_modified(), "the commit point ends the buffering");

        let mut back = [0u8; 8];
        space.read_at(3 * EXTENT + 10, &mut back).expect("reads");
        assert_eq!(&back, b"authored");
        // The extent nothing was written to still costs nothing and still
        // reads as the blank it is.
        let mut untouched = [0xffu8; 16];
        space.read_at(EXTENT, &mut untouched).expect("reads");
        assert_eq!(untouched, [0u8; 16]);
    }

    #[test]
    fn nothing_is_kept_that_a_rollback_discards() {
        let mut space = AuthoredSpace::new(2 * EXTENT, 1 << 20);
        space.write_at(16, b"committed").expect("writes");
        space.commit().expect("commits");
        space.write_at(64, b"doomed").expect("writes");
        space.rollback();
        assert!(!space.is_modified());

        let mut kept = [0u8; 9];
        space.read_at(16, &mut kept).expect("reads");
        assert_eq!(&kept, b"committed", "the committed content survives");
        let mut gone = [0xffu8; 6];
        space.read_at(64, &mut gone).expect("reads");
        assert_eq!(&gone, &[0u8; 6], "and the rolled-back write never landed");
    }

    #[test]
    fn a_read_past_the_authored_coordinates_is_refused() {
        let mut space = AuthoredSpace::new(1024, 1 << 20);
        let mut buf = [0u8; 32];
        assert!(space.read_at(1000, &mut buf).is_err());
        assert!(space.write_at(1000, &buf).is_err());
    }
}
