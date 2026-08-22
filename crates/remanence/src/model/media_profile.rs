// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The media-profile seam (P14): where a medium's own passive
//! compatibility facts are declared, and where the catalog of
//! **articles** that holds them lives.
//!
//! An article is the physical substrate — what a medium *is* — and it is
//! one of D19's three facts. The other two live elsewhere: the recording
//! in the device type ([`crate::model::device_type`], which composes an article
//! and restates none of its facts), and the drive's behavior in the P30
//! drive profile. `article()` is the caller-facing spelling of what this
//! catalog names.
//!
//! A media instance is the independent mutable state between image
//! formats and drives — the block state a [`crate::StorageDevice`] presents, the
//! circular pulse streams a [`crate::flux::medium::FluxMedium`] holds.
//! Each of them **names** one of the profiles below, and the recorded
//! contents stay with the instance: nothing here holds a byte, a pulse,
//! or a location.
//!
//! **A profile is passive, and that is what makes the catalog
//! declarative.** There is no probe here, no grammar, no behavior and no
//! seam to descend through. A media type states what the medium *is* —
//! its form, its coating, the holes punched in it, which way its
//! write-protect mechanism reads — and answers nothing about how an
//! image format encodes it, how a drive clocks it, or how far a hardware
//! emulation goes. Recognition belongs to the image-format adapter
//! (P12), the recording conventions and read-channel rules to the drive
//! profile (P30), and timed causality to a hardware emulation (P15). A
//! catalog entry that could choose any of those would be a language for
//! behavior wearing a table's clothes.
//!
//! **Families own their representation.** Flexible magnetic media,
//! optical media and logical-block media have no fact in common — a
//! coercivity is meaningless for the last two, a track pitch in
//! nanometres for the other two, and a block size for the first two — so
//! the facts are family-specific by construction rather than one schema
//! with most of its fields empty. Four families are claimed (P3) and a
//! media type outside them is refused by name.
//!
//! **The fourth is virtual, and it proves the rule rather than bending
//! it.** An archive is independent recorded state with no physical
//! article behind it, held by no drive — which is exactly what P14's own
//! sentence describes, media being the independent mutable state
//! *between* image formats and drives. It has no form factor, no
//! coercivity, no addressable unit and no hole to declare, so what its
//! family declares instead is the **native vantage**: a namespace, where
//! every physical family's is a space.
//!
//! **What is deliberately absent.** How many surfaces are *recorded* is
//! the drive profile's declaration and the image format's geometry, not
//! the medium's: the same 5.25-inch disk is one recorded surface in a
//! 1541 and two in a drive that records both, and nothing in a capture
//! or an image establishes which certification the physical disk
//! carried. Density, encoding, track and sector counts are absent for
//! the same reason — they are what was recorded on the medium, which
//! belongs to the instance and to the seams above it.
//!
//! Every item is crate-private, as the drive-profile seam beside it is:
//! a media type reaches a caller as the name a medium answers with, and
//! the facts stay behind that name until something outside the library
//! genuinely needs one.
#![allow(dead_code)]

use crate::error::{Error, Result};

/// The media families the library claims (P3).
///
/// A family is the unit of representation here: its facts are its own,
/// and nothing above it merges two families into one schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MediaFamily {
    /// Flexible magnetic media — the removable disk a drive's head
    /// touches.
    FlexibleMagnetic,
    /// Logical-block media — geometry-opaque blocks addressed by
    /// number, with no cylinder, head, track, recording or mechanism
    /// claim (P23).
    LogicalBlock,
    /// Optical media — the disc a drive's laser reads, whose
    /// compatibility is a matter of optics rather than of magnetism.
    ///
    /// **The family declares the article and nothing recorded on it.**
    /// Sessions, tracks, index points, audio and subchannels are what
    /// was *recorded*, which belongs to the instance and to the optical
    /// state model this release does not claim; what a blank disc in
    /// its sleeve carries is its size, the spiral it was manufactured
    /// to, and whether anything can write it.
    Optical,
    /// **Virtual** media — independent recorded state with no physical
    /// article behind it, held by no drive. P14's own definition already
    /// describes it: media is the independent mutable state between
    /// image formats and drives, and being independent of drives is the
    /// point. Its members carry no physical fact, so what a family
    /// declares here is its native vantage instead.
    Virtual,
}

impl MediaFamily {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::FlexibleMagnetic => "flexible-magnetic",
            Self::LogicalBlock => "logical-block",
            Self::Optical => "optical",
            Self::Virtual => "virtual",
        }
    }
}

/// The nominal size of a flexible disk's jacket.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FormFactor {
    Inch5_25,
    Inch3_5,
}

impl FormFactor {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Inch5_25 => "5.25-inch",
            Self::Inch3_5 => "3.5-inch",
        }
    }
}

/// How the medium itself divides a revolution.
///
/// This is a fact about the disk rather than about anything recorded on
/// it: a hard-sectored disk carries its division as holes punched in the
/// media, which every drive sees whether or not it is formatted at all.
/// A soft-sectored disk carries no such division and leaves the whole
/// question to the recording.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Sectoring {
    Soft,
    Hard {
        /// How many sector holes are punched, evenly spaced around the
        /// hub.
        sector_holes: u32,
    },
}

impl Sectoring {
    /// The sector holes the medium carries — zero for soft-sectored
    /// media, which carries none rather than an unknown number.
    pub(crate) fn sector_holes(self) -> u32 {
        match self {
            Self::Soft => 0,
            Self::Hard { sector_holes } => sector_holes,
        }
    }
}

/// The medium's write-protect mechanism, and which way it reads.
///
/// The sense is the whole point of declaring it: the same notch means
/// opposite things on different media, so a mechanism without its sense
/// would leave every reader to assume one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WriteProtect {
    /// A notch cut in the jacket edge, read by the drive's sensor.
    Notch { protected_when_covered: bool },
    /// A sliding tab in the shell that opens or closes a window, read by
    /// the drive's sensor. The sense runs the other way from a notch: a
    /// 3.5-inch disk is protected when its window is *open*.
    Slider { protected_when_open: bool },
}

/// The passive compatibility facts of one flexible magnetic media type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FlexibleMagnetic {
    pub(crate) form_factor: FormFactor,
    /// The coating's coercivity in oersteds — what a drive's write
    /// current has to match, and the reason two disks of one size are
    /// not interchangeable.
    pub(crate) coercivity_oersteds: u32,
    /// The track density the medium is made to, in tracks per inch.
    pub(crate) tracks_per_inch: u32,
    pub(crate) sectoring: Sectoring,
    /// How many index holes are punched. Whether any drive *observes*
    /// one is the drive profile's declaration and a different fact
    /// entirely — the 1541 is served media with an index hole and has
    /// no sensor to see it.
    pub(crate) index_holes: u32,
    pub(crate) write_protect: WriteProtect,
}

/// The passive compatibility facts of one logical-block media type.
///
/// There is exactly one, deliberately: a logical-block medium is
/// geometry-opaque by definition, so the addressable unit is the only
/// thing about it a controller can be compatible or incompatible with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LogicalBlock {
    pub(crate) block_bytes: u64,
}

/// The nominal diameter of an optical disc.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DiscSize {
    Millimetre120,
}

impl DiscSize {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Millimetre120 => "120 mm",
        }
    }
}

/// How the article came to carry marks, and therefore whether anything
/// can put more there.
///
/// It is the optical family's answer to the flexible one's write-protect
/// notch, and it differs in kind rather than in degree: a notch is a
/// mechanism whose sense a drive reads and a user can defeat with tape,
/// where a pressed disc has no write mechanism at all. Recordable and
/// rewritable articles exist and are not enrolled — the catalog is an
/// enumerated claim (P3), and this release is served the pressed one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DiscRecording {
    /// Stamped from a glass master at manufacture. There is nothing to
    /// write with and nothing to erase.
    Pressed,
}

/// The passive compatibility facts of one optical media type.
///
/// The two dimensions below are the optical family's coercivity: they
/// are why a drive built for one article cannot read another of the same
/// diameter, and they are published facts about a manufactured disc
/// rather than anything an image or a capture may establish.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Optical {
    pub(crate) disc_size: DiscSize,
    /// The pitch of the manufactured spiral, in nanometres — what a
    /// drive's optics have to track.
    pub(crate) track_pitch_nm: u32,
    /// The wavelength the article is made to be read at, in nanometres.
    pub(crate) read_wavelength_nm: u32,
    pub(crate) recording: DiscRecording,
}

/// The passive facts of one virtual media type.
///
/// There is exactly one fact, and it is not a physical one: a virtual
/// medium has no form factor, no coercivity and no addressable unit to
/// be compatible or incompatible with. What distinguishes its members is
/// the **native vantage** — the one way its content is reached at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Virtual {
    /// `"namespace"` for an archive, where every physical family's is a
    /// space. It is stated rather than assumed because it is the whole
    /// of what the family declares.
    pub(crate) native_vantage: &'static str,
}

/// One media type's facts, in its own family's vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MediaFacts {
    FlexibleMagnetic(FlexibleMagnetic),
    LogicalBlock(LogicalBlock),
    Optical(Optical),
    Virtual(Virtual),
}

impl MediaFacts {
    pub(crate) fn family(&self) -> MediaFamily {
        match self {
            Self::FlexibleMagnetic(_) => MediaFamily::FlexibleMagnetic,
            Self::LogicalBlock(_) => MediaFamily::LogicalBlock,
            Self::Optical(_) => MediaFamily::Optical,
            Self::Virtual(_) => MediaFamily::Virtual,
        }
    }
}

/// One media type: its identity, and the passive facts of the medium it
/// names, with the published description they were declared from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MediaProfile {
    pub(crate) id: &'static str,
    pub(crate) name: &'static str,
    /// Where every declared fact came from. A media type carries this
    /// for the same reason a drive profile does: these are published
    /// facts about a manufactured article, never values an image or a
    /// capture is permitted to establish.
    pub(crate) provenance: &'static str,
    pub(crate) facts: MediaFacts,
}

impl MediaProfile {
    pub(crate) fn family(&self) -> MediaFamily {
        self.facts.family()
    }

    /// The flexible magnetic facts, or `None` where this is another
    /// family's medium. A caller in one family's code asks for its own
    /// facts and gets an answer or nothing — never another family's
    /// fields under its own names.
    pub(crate) fn flexible_magnetic(&self) -> Option<&FlexibleMagnetic> {
        match &self.facts {
            MediaFacts::FlexibleMagnetic(facts) => Some(facts),
            _ => None,
        }
    }

    pub(crate) fn logical_block(&self) -> Option<&LogicalBlock> {
        match &self.facts {
            MediaFacts::LogicalBlock(facts) => Some(facts),
            _ => None,
        }
    }

    pub(crate) fn optical(&self) -> Option<&Optical> {
        match &self.facts {
            MediaFacts::Optical(facts) => Some(facts),
            _ => None,
        }
    }

    pub(crate) fn virtual_media(&self) -> Option<&Virtual> {
        match &self.facts {
            MediaFacts::Virtual(facts) => Some(facts),
            _ => None,
        }
    }
}

/// Soft-sectored 5.25-inch media: what a 1541 is served, and what every
/// soft-sectored drive of that size and coating takes.
pub(crate) static FLEXIBLE_5_25_SOFT: MediaProfile = MediaProfile {
    id: "flexible-5.25-soft",
    name: "5.25-inch soft-sectored flexible disk",
    provenance: "declared from the published 5.25-inch flexible media class: a \
                 300-oersted coating made to 48 tracks per inch, one index hole, \
                 no sector holes, and a jacket notch that protects the disk when \
                 it is covered",
    facts: MediaFacts::FlexibleMagnetic(FlexibleMagnetic {
        form_factor: FormFactor::Inch5_25,
        coercivity_oersteds: 300,
        tracks_per_inch: 48,
        sectoring: Sectoring::Soft,
        index_holes: 1,
        write_protect: WriteProtect::Notch {
            protected_when_covered: true,
        },
    }),
};

/// Ten-sector hard-sectored 5.25-inch media: what an H17 drive is
/// served.
///
/// The ten holes are the medium's own division of the revolution, which
/// is why the format above it records ten records to a track rather than
/// choosing a number.
pub(crate) static FLEXIBLE_5_25_HARD_10: MediaProfile = MediaProfile {
    id: "flexible-5.25-hard-10",
    name: "5.25-inch ten-sector hard-sectored flexible disk",
    provenance: "declared from the published H17 media conventions: the same \
                 300-oersted 48-tracks-per-inch 5.25-inch article as the \
                 soft-sectored disk, punched with ten evenly spaced sector holes \
                 and one index hole between two of them",
    facts: MediaFacts::FlexibleMagnetic(FlexibleMagnetic {
        form_factor: FormFactor::Inch5_25,
        coercivity_oersteds: 300,
        tracks_per_inch: 48,
        sectoring: Sectoring::Hard { sector_holes: 10 },
        index_holes: 1,
        write_protect: WriteProtect::Notch {
            protected_when_covered: true,
        },
    }),
};

/// High-density 5.25-inch media: what a PC's 1.2 MB drive is served.
///
/// It is the same jacket, notch and index hole as the double-density
/// disk above, and a different article all the same: the coating takes
/// twice the write current and the disk is made to 96 tracks per inch
/// rather than 48. That is why a 1.2 MB drive cannot reliably write a
/// 360 KB disk and why a 360 KB drive reads nothing off this one — and
/// why the two are separate entries rather than one with a density
/// flag.
pub(crate) static FLEXIBLE_5_25_HD: MediaProfile = MediaProfile {
    id: "flexible-5.25-hd",
    name: "5.25-inch high-density flexible disk",
    provenance: "declared from the published 5.25-inch high-density media class: a \
                 600-oersted coating made to 96 tracks per inch in the same jacket \
                 as the double-density disk, one index hole, no sector holes, and \
                 a jacket notch that protects the disk when it is covered",
    facts: MediaFacts::FlexibleMagnetic(FlexibleMagnetic {
        form_factor: FormFactor::Inch5_25,
        coercivity_oersteds: 600,
        tracks_per_inch: 96,
        sectoring: Sectoring::Soft,
        index_holes: 1,
        write_protect: WriteProtect::Notch {
            protected_when_covered: true,
        },
    }),
};

/// High-density 3.5-inch media: what a PC's 1.44 MB drive is served.
///
/// The index is the hub's rather than the disk's: a 3.5-inch cookie has
/// no hole punched in it, and the drive derives the index pulse from the
/// metal hub it spins, which is why `index_holes` is zero here while the
/// drive profile still declares that it observes an index.
pub(crate) static FLEXIBLE_3_5_HD: MediaProfile = MediaProfile {
    id: "flexible-3.5-hd",
    name: "3.5-inch high-density flexible disk",
    provenance: "declared from the published 3.5-inch high-density media class: a \
                 720-oersted coating made to 135 tracks per inch in a rigid shell, \
                 a metal hub the drive keys to and takes its index from, no holes in \
                 the disk itself, and a sliding tab that protects the disk when its \
                 window is open",
    facts: MediaFacts::FlexibleMagnetic(FlexibleMagnetic {
        form_factor: FormFactor::Inch3_5,
        coercivity_oersteds: 720,
        tracks_per_inch: 135,
        sectoring: Sectoring::Soft,
        index_holes: 0,
        write_protect: WriteProtect::Slider {
            protected_when_open: true,
        },
    }),
};

/// The logical-block medium every block-active image presents.
pub(crate) static LOGICAL_BLOCK_512: MediaProfile = MediaProfile {
    id: "logical-block-512",
    name: "512-byte logical-block medium",
    provenance: "declared from the block addressing every claimed block image \
                 format and the MBR schema above them are written against: \
                 512-byte blocks addressed by number, with no cylinder, head, \
                 track, recording or mechanism fact of any kind",
    facts: MediaFacts::LogicalBlock(LogicalBlock { block_bytes: 512 }),
};

/// The pressed 120 mm optical disc: what a CD-ROM drive is served.
///
/// **It declares the disc and not the disc's content.** How many
/// sessions were burned, where the tracks begin, which of them carry
/// audio and what the subchannels say are facts about a recording, and
/// they belong to the instance and to the optical state model this
/// release does not claim — the same line the flexible entries draw when
/// they decline to state a track count.
pub(crate) static OPTICAL_120_PRESSED: MediaProfile = MediaProfile {
    id: "optical-120-pressed",
    name: "120 mm pressed optical disc",
    provenance: "declared from the published compact-disc physical specification: \
                 a 120-millimetre disc carrying a spiral of 1.6-micrometre pitch \
                 stamped at manufacture and read at 780 nanometres, with no \
                 session, track, index, audio or subchannel fact of any kind, \
                 those being what was recorded rather than what the disc is",
    facts: MediaFacts::Optical(Optical {
        disc_size: DiscSize::Millimetre120,
        track_pitch_nm: 1600,
        read_wavelength_nm: 780,
        recording: DiscRecording::Pressed,
    }),
};

/// The virtual article: independent recorded state with no physical
/// substrate behind it, whose content is reached by name and not by
/// position — an archive's.
///
/// A zip's byte extent is its *encoding* (P13), not a model space —
/// there is no meaningful "sector 5 of a zip" — which is why the one
/// fact declared here is the vantage.
pub(crate) static VIRTUAL: MediaProfile = MediaProfile {
    id: "virtual",
    name: "virtual medium",
    provenance: "declared from what an archive is rather than from a published                  article: independent recorded state held by no drive, whose                  grammar names its content and whose bytes are its encoding",
    facts: MediaFacts::Virtual(Virtual {
        native_vantage: "namespace",
    }),
};

/// The authored article: state an author created whole, with no
/// manufactured article behind it and no drive that ever held one.
///
/// It sits in the virtual family for the same reason the archive does —
/// P14's own definition already covers it, media being the independent
/// mutable state between image formats and drives — and it differs from
/// the archive in exactly the fact that family declares: its native
/// vantage is a **space**, because what the author states is the
/// recording's own coordinates and its content is reached by position.
///
/// No physical fact is declared here and none is invented: an authored
/// blank has no coercivity, no form factor and no punched hole, because
/// nobody manufactured it. A caller authoring the *manufactured* article
/// names that article instead ([`crate::NewMedia`]'s blank article
/// kinds), and gets its published facts rather than these.
pub(crate) static AUTHORED: MediaProfile = MediaProfile {
    id: "authored",
    name: "authored medium",
    provenance: "declared from what authorship is rather than from a published \
                 article: state created whole by an author, held by no drive and \
                 recorded by no device, whose coordinates are the author's own \
                 facts and whose content is reached by position",
    facts: MediaFacts::Virtual(Virtual {
        native_vantage: "space",
    }),
};

/// The enrolled articles. Adding one changes its declaration, its
/// tests, and this list — nothing else, because there is no behavior
/// here to wire up.
static ENROLLED: [&MediaProfile; 8] = [
    &FLEXIBLE_5_25_SOFT,
    &FLEXIBLE_5_25_HARD_10,
    &FLEXIBLE_5_25_HD,
    &FLEXIBLE_3_5_HD,
    &LOGICAL_BLOCK_512,
    &OPTICAL_120_PRESSED,
    &VIRTUAL,
    &AUTHORED,
];

pub(crate) fn enrolled() -> &'static [&'static MediaProfile] {
    &ENROLLED
}

/// Resolves an article by name, refusing one the library does not
/// claim (P3).
///
/// It exists for the boundaries where an article arrives as text —
/// nothing inside the crate names one this way, holding the static
/// instead.
pub(crate) fn by_id(id: &str) -> Result<&'static MediaProfile> {
    ENROLLED
        .iter()
        .copied()
        .find(|profile| profile.id == id)
        .ok_or_else(|| {
            let claimed: Vec<&str> = ENROLLED.iter().map(|profile| profile.id).collect();
            Error::unsupported(format!(
                "no article named '{id}' is claimed; this release claims {}",
                claimed.join(", ")
            ))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_enrolled_type_is_named_once_and_carries_its_provenance() {
        for profile in enrolled() {
            assert!(!profile.id.is_empty(), "an entry with no id names nothing");
            assert!(!profile.name.is_empty(), "{} has no name", profile.id);
            assert!(
                !profile.provenance.is_empty(),
                "{} declares facts from nowhere",
                profile.id
            );
            assert_eq!(
                by_id(profile.id).expect("an enrolled type resolves").id,
                profile.id,
            );
        }
        let mut ids: Vec<&str> = enrolled().iter().map(|profile| profile.id).collect();
        let count = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), count, "two entries share an id");
    }

    #[test]
    fn the_virtual_family_is_two_entries_and_the_vantage_is_what_parts_them() {
        // The family declares one fact, so two members of it differ in
        // that fact and in nothing else: an archive is reached by the
        // names it holds and an authored blank by position.
        assert_eq!(AUTHORED.family(), MediaFamily::Virtual);
        assert_eq!(
            AUTHORED
                .virtual_media()
                .expect("its own family's facts")
                .native_vantage,
            "space"
        );
        assert_eq!(
            VIRTUAL
                .virtual_media()
                .expect("its own family's facts")
                .native_vantage,
            "namespace"
        );
        assert!(
            AUTHORED.flexible_magnetic().is_none() && AUTHORED.logical_block().is_none(),
            "nobody manufactured an authored medium, so it answers no \
             physical question"
        );
    }

    #[test]
    fn the_virtual_family_declares_a_vantage_and_no_physical_fact() {
        // P14's amendment: an archive is a medium, and its one family
        // fact is the native vantage. Asking it a physical question is
        // asking another family's, which is what the accessors refuse.
        let archive = VIRTUAL.virtual_media().expect("its own family's facts");
        assert_eq!(archive.native_vantage, "namespace");
        assert_eq!(VIRTUAL.family(), MediaFamily::Virtual);
        assert!(
            VIRTUAL.flexible_magnetic().is_none(),
            "an archive answers no coercivity question"
        );
        assert!(
            VIRTUAL.logical_block().is_none(),
            "and no addressable-unit question either"
        );
        assert!(
            LOGICAL_BLOCK_512.virtual_media().is_none(),
            "and a physical medium declares no vantage here"
        );

        // Every physical family's vantage is a space, and the split that
        // matters is space-native against namespace-native.
        for physical in [
            &FLEXIBLE_5_25_SOFT,
            &FLEXIBLE_5_25_HARD_10,
            &FLEXIBLE_5_25_HD,
            &FLEXIBLE_3_5_HD,
            &LOGICAL_BLOCK_512,
            &OPTICAL_120_PRESSED,
        ] {
            assert_ne!(physical.family(), MediaFamily::Virtual, "{}", physical.id);
        }
    }

    #[test]
    fn the_optical_article_declares_the_disc_and_nothing_recorded_on_it() {
        // D19's test for an article fact: it holds of a blank disc in
        // its sleeve. A pressed disc's size, spiral and stamping do; a
        // session, a track and an audio flag are what somebody put on
        // it, and the family declares none of them.
        let disc = OPTICAL_120_PRESSED
            .optical()
            .expect("its own family's facts");
        assert_eq!(disc.disc_size, DiscSize::Millimetre120);
        assert_eq!(disc.track_pitch_nm, 1600);
        assert_eq!(disc.read_wavelength_nm, 780);
        assert_eq!(
            disc.recording,
            DiscRecording::Pressed,
            "a pressed disc has no write mechanism at all, where a flexible \
             disk has one whose sense the family declares"
        );
        assert_eq!(OPTICAL_120_PRESSED.family(), MediaFamily::Optical);

        // Families own their representation: a disc answers no
        // coercivity question, and no block-size one either — what a
        // recording addresses in belongs to the recording.
        assert!(
            OPTICAL_120_PRESSED.flexible_magnetic().is_none()
                && OPTICAL_120_PRESSED.logical_block().is_none()
                && OPTICAL_120_PRESSED.virtual_media().is_none(),
            "an optical disc answers only its own family's questions"
        );
        assert!(
            LOGICAL_BLOCK_512.optical().is_none(),
            "and a logical-block medium answers no optical one"
        );
    }

    #[test]
    fn an_unclaimed_article_is_refused_by_name() {
        // P3: the catalog is an enumerated claim. 8-inch media is the
        // obvious next flexible entry and naming it must refuse rather
        // than approximate it from the 5.25-inch declaration.
        let error = by_id("flexible-8-soft").expect_err("refused");
        let message = error.to_string();
        assert!(
            message.contains("flexible-8-soft"),
            "names what was asked: {message}"
        );
        assert!(
            message.contains("flexible-5.25-soft"),
            "names what is claimed: {message}"
        );
    }

    #[test]
    fn a_familys_facts_are_reached_only_through_its_own_family() {
        let flexible = FLEXIBLE_5_25_SOFT
            .flexible_magnetic()
            .expect("its own family's facts");
        assert_eq!(flexible.form_factor, FormFactor::Inch5_25);
        assert_eq!(flexible.coercivity_oersteds, 300);
        assert_eq!(flexible.tracks_per_inch, 48);
        assert_eq!(flexible.sectoring, Sectoring::Soft);
        assert_eq!(flexible.sectoring.sector_holes(), 0);
        assert_eq!(flexible.index_holes, 1);
        assert_eq!(
            flexible.write_protect,
            WriteProtect::Notch {
                protected_when_covered: true
            }
        );
        assert!(
            FLEXIBLE_5_25_SOFT.logical_block().is_none(),
            "a flexible disk answers no block question"
        );
    }

    #[test]
    fn the_two_5_25_inch_soft_sectored_articles_differ_in_coating_and_pitch_alone() {
        // Same jacket, same notch, same single index hole — and a disk
        // a 1541 cannot write, because the coating wants twice the
        // current and the tracks sit half as far apart. Those two facts
        // are the whole of what parts the entries, and they are what a
        // drive's compatibility turns on.
        let double = FLEXIBLE_5_25_SOFT
            .flexible_magnetic()
            .expect("its own family's facts");
        let high = FLEXIBLE_5_25_HD
            .flexible_magnetic()
            .expect("its own family's facts");
        assert_eq!(high.form_factor, double.form_factor);
        assert_eq!(high.sectoring, double.sectoring);
        assert_eq!(high.index_holes, double.index_holes);
        assert_eq!(high.write_protect, double.write_protect);
        assert_eq!(high.coercivity_oersteds, 600);
        assert_eq!(high.tracks_per_inch, 96);
        assert_eq!(double.coercivity_oersteds, 300);
        assert_eq!(double.tracks_per_inch, 48);
    }

    #[test]
    fn the_high_density_article_takes_its_index_from_the_hub_and_protects_when_open() {
        let flexible = FLEXIBLE_3_5_HD
            .flexible_magnetic()
            .expect("its own family's facts");
        assert_eq!(flexible.form_factor, FormFactor::Inch3_5);
        assert_eq!(flexible.form_factor.name(), "3.5-inch");
        assert_eq!(flexible.coercivity_oersteds, 720);
        assert_eq!(flexible.tracks_per_inch, 135);
        assert_eq!(flexible.sectoring, Sectoring::Soft);
        // No hole in the cookie: the drive takes its index from the hub.
        assert_eq!(flexible.index_holes, 0);
        // The sense runs the other way from a 5.25-inch notch.
        assert_eq!(
            flexible.write_protect,
            WriteProtect::Slider {
                protected_when_open: true
            }
        );
        assert_eq!(
            by_id("flexible-3.5-hd").expect("enrolled").id,
            FLEXIBLE_3_5_HD.id
        );

        let block = LOGICAL_BLOCK_512
            .logical_block()
            .expect("its own family's facts");
        assert_eq!(block.block_bytes, 512);
        assert!(
            LOGICAL_BLOCK_512.flexible_magnetic().is_none(),
            "a logical-block medium answers no coercivity question"
        );

        assert_eq!(
            FLEXIBLE_5_25_SOFT.family(),
            MediaFamily::FlexibleMagnetic,
            "the family follows the facts rather than being declared twice"
        );
        assert_eq!(LOGICAL_BLOCK_512.family(), MediaFamily::LogicalBlock);
    }

    #[test]
    fn the_hard_sectored_disk_carries_its_own_division_of_the_revolution() {
        let hard = FLEXIBLE_5_25_HARD_10
            .flexible_magnetic()
            .expect("flexible facts");
        assert_eq!(hard.sectoring, Sectoring::Hard { sector_holes: 10 });
        assert_eq!(hard.index_holes, 1, "the index hole is not a sector hole");

        // The two 5.25-inch entries differ in exactly one fact: the
        // holes. Everything else is the same manufactured article, and
        // stating it twice differently would be two answers about one
        // disk.
        let soft = FLEXIBLE_5_25_SOFT
            .flexible_magnetic()
            .expect("flexible facts");
        assert_eq!(hard.form_factor, soft.form_factor);
        assert_eq!(hard.coercivity_oersteds, soft.coercivity_oersteds);
        assert_eq!(hard.tracks_per_inch, soft.tracks_per_inch);
        assert_eq!(hard.write_protect, soft.write_protect);
    }
}
