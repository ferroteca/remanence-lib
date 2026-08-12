// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The device-family catalog (P32): what a machine's slots can be, stated
//! as data.
//!
//! A family entry is **as concrete as the machine fact it asserts** — a
//! Commodore 1541, not "some floppy" — and each entry states what it is a
//! kind of. That lineage is data in this catalog, never a type hierarchy,
//! mirroring the drive-profile catalog one seam down (P30) and the
//! media-type catalog beside it (P14).
//!
//! **Interior names classify; only concrete entries instantiate.** A
//! device added as "some floppy" would exist in no machine: it declares
//! no media for [`crate::DeviceView::insert`] to check a medium
//! against, and no slot for a namespace composer to reason over. Asking
//! whether a family *is a* floppy drive is a query, and it is answered
//! here; asking a machine for one is refused by name.
//!
//! Three facts hang off a concrete entry and none off an interior one:
//!
//! - its **slot prefix**, the family half of every attachment identity in
//!   it (`hdd0`, `cbmfloppy0`). It is digit-free and unique across the
//!   catalog, because an identity parses as prefix-then-index;
//! - the **media types it accepts** (P14), by which a medium that belongs
//!   in another drive is refused naming both sides rather than read as
//!   something it is not; and
//! - its **claimed flux path** where the family has one (P22) — the drive
//!   profile whose declared stepping, rotation and read channel govern
//!   the recording. A family that claims none is ordinary, not deficient:
//!   a logical-block device has no recording path to declare, which is
//!   the whole point of the family.
//!
//! What is deliberately absent is an addressing nature. The pledged P32
//! amendment puts that on the *device*, declared when it is created,
//! precisely because no family owns one — a hard drive answers both CHS
//! and LBA depending on the command issued.

use std::fmt;

use crate::drive_profile::{self, DriveProfile};
use crate::error::{Error, Result};
use crate::media_profile::{
    ARCHIVE, FLEXIBLE_5_25_HARD_10, FLEXIBLE_5_25_SOFT, LOGICAL_BLOCK_512, MediaProfile,
};

/// One entry in the catalog: a family, its place in the lineage, and
/// what it declares.
///
/// Private, as the media-type and drive-profile entries are. A family
/// reaches a caller as [`DeviceFamily`], which answers for the facts a
/// caller has a use for and keeps the rest behind the name.
pub(crate) struct FamilyEntry {
    id: &'static str,
    name: &'static str,
    /// What this family is a kind of — `None` only for the root.
    kind_of: Option<&'static FamilyEntry>,
    /// Where the declaration came from. An entry asserts a machine fact,
    /// so it carries its source for the same reason a drive profile and
    /// a media type do.
    provenance: &'static str,
    /// The family half of an attachment identity, and the marker of a
    /// concrete entry: `None` is what makes a name classify rather than
    /// instantiate.
    slot_prefix: Option<&'static str>,
    /// The media types a device of this family accepts (P14). Empty for
    /// an interior name, which declares nothing a medium could be
    /// checked against.
    media: &'static [&'static MediaProfile],
    /// The drive profile governing this family's recording path where it
    /// claims one (P22).
    flux_path: Option<&'static DriveProfile>,
}

/// A storage-device family: an entry in the catalog above, held by
/// value.
///
/// It is a handle on one static entry rather than a variant, which is
/// what lets the lineage be data. Two handles are equal exactly when they
/// name the same entry.
#[derive(Clone, Copy)]
pub struct DeviceFamily(&'static FamilyEntry);

impl DeviceFamily {
    /// The root of the lineage. Classifies everything and instantiates
    /// nothing.
    pub const STORAGE_DEVICE: Self = Self(&STORAGE_DEVICE);

    /// Floppy drives — the interior name every claimed flexible-media
    /// drive descends from.
    pub const FLOPPY_DRIVE: Self = Self(&FLOPPY_DRIVE);

    /// Commodore's floppy drives, grouped because they share a recording
    /// path and a serial-bus lineage the rest of the tree does not.
    pub const CBM_FLOPPY_DRIVE: Self = Self(&CBM_FLOPPY_DRIVE);

    /// The Commodore 1541 — concrete, and the family whose claimed flux
    /// path the delivered drive profile declares.
    pub const COMMODORE_1541: Self = Self(&COMMODORE_1541);

    /// The Heathkit H-17 floppy drive — concrete, served the ten-sector
    /// hard-sectored disk an H8D image records.
    pub const HEATHKIT_H17: Self = Self(&HEATHKIT_H17);

    /// A hard disk — concrete, and as concrete as the machine fact goes:
    /// the family deliberately hides everything beneath its block
    /// interface (P15), so a model number would be an assertion the
    /// caller cannot check and the library never reads.
    pub const HARD_DISK: Self = Self(&HARD_DISK);

    /// The archive slot — concrete, and **virtual**: an attachment with
    /// no mechanism at all, which is the whole of what it is. An archive
    /// medium is held by no drive (P14), so what it loads into is a
    /// receiver rather than a machine's hardware, and it is a device for
    /// the same reason every other medium's holder is: the caller never
    /// holds a medium outside one.
    pub const ARCHIVE_DEVICE: Self = Self(&ARCHIVE_DEVICE);

    /// The family's stable cross-language spelling.
    pub fn id(self) -> &'static str {
        self.0.id
    }

    /// The family's name, fit to show a user beside the slot it fills.
    pub fn name(self) -> &'static str {
        self.0.name
    }

    /// Where this entry's declaration came from.
    pub fn provenance(self) -> &'static str {
        self.0.provenance
    }

    /// What this family is a kind of — `None` for the root of the
    /// lineage.
    pub fn kind_of(self) -> Option<Self> {
        self.0.kind_of.map(Self)
    }

    /// This family and everything it is a kind of, nearest first.
    pub fn lineage(self) -> Vec<Self> {
        let mut line = vec![self];
        let mut at = self;
        while let Some(parent) = at.kind_of() {
            line.push(parent);
            at = parent;
        }
        line
    }

    /// Whether this family is `other`, or a kind of it. Reflexive: a
    /// Commodore 1541 is a Commodore 1541, and it is also a floppy drive.
    pub fn is_a(self, other: Self) -> bool {
        self.lineage().contains(&other)
    }

    /// Whether a device of this family can exist in a machine.
    ///
    /// Interior names classify and never instantiate, which is the
    /// refusal `add_device` makes: vagueness in a machine fact declares
    /// nothing a medium or a composer could be checked against.
    pub fn is_concrete(self) -> bool {
        self.0.slot_prefix.is_some()
    }

    /// The family half of every attachment identity in it — `hdd` for
    /// `hdd0` — or `None` for an interior name, which names no slot.
    pub fn slot_prefix(self) -> Option<&'static str> {
        self.0.slot_prefix
    }

    /// The media types a device of this family accepts (P14), by their
    /// stable spellings. Empty for an interior name.
    pub fn accepted_media(self) -> Vec<&'static str> {
        self.0.media.iter().map(|media| media.id).collect()
    }

    /// Whether this family accepts `media`.
    pub(crate) fn accepts(self, media: &'static MediaProfile) -> bool {
        self.0.media.iter().any(|claimed| claimed.id == media.id)
    }

    /// Every family a device could hold `media` in, derived by asking the
    /// families themselves.
    ///
    /// It is a **query over the declarations**, not a second list: a
    /// family states the media it accepts (P14, D19's direction), and
    /// this reads that statement the other way round. Interior names are
    /// absent because they accept nothing and instantiate nothing, so an
    /// empty answer means no drive this release claims is served the
    /// article — an outcome, not a defect in the medium.
    pub(crate) fn accepting(media: &'static MediaProfile) -> Vec<Self> {
        Self::concrete()
            .into_iter()
            .filter(|family| family.accepts(media))
            .collect()
    }

    /// The drive profile this family claims as its recording path (P22),
    /// by its stable spelling, or `None` where the family claims none.
    pub fn flux_path(self) -> Option<&'static str> {
        self.0.flux_path.map(|profile| profile.id)
    }

    /// Every enrolled family, interior names among them.
    pub fn enrolled() -> Vec<Self> {
        ENROLLED.iter().copied().map(Self).collect()
    }

    /// Every family a device can actually be added as.
    pub fn concrete() -> Vec<Self> {
        Self::enrolled()
            .into_iter()
            .filter(|family| family.is_concrete())
            .collect()
    }

    /// Resolves a family by its stable spelling, refusing one this
    /// release does not claim by name (P3).
    ///
    /// Rust callers hold the constants above; this is for the C and
    /// Python surfaces, where a family arrives as text and the refusal
    /// has to happen at that boundary. An interior name resolves here —
    /// classifying is what it is for — and is refused where a device is
    /// asked for.
    pub fn from_id(id: &str) -> Result<Self> {
        ENROLLED
            .iter()
            .copied()
            .find(|entry| entry.id == id)
            .map(Self)
            .ok_or_else(|| {
                let claimed: Vec<&str> = ENROLLED.iter().map(|entry| entry.id).collect();
                Error::unsupported(format!(
                    "no storage-device family named '{id}' is claimed; this \
                     release claims {}",
                    claimed.join(", ")
                ))
            })
    }

    /// Resolves the concrete family that owns a slot prefix — `hdd`
    /// naming the hard disk — refusing an unclaimed one by name.
    pub(crate) fn by_slot_prefix(prefix: &str) -> Result<Self> {
        ENROLLED
            .iter()
            .copied()
            .find(|entry| entry.slot_prefix == Some(prefix))
            .map(Self)
            .ok_or_else(|| {
                let claimed: Vec<&str> = ENROLLED.iter().filter_map(|entry| entry.slot_prefix).collect();
                Error::unsupported(format!(
                    "no storage-device family takes the slot prefix '{prefix}'; \
                     this release claims {}",
                    claimed.join(", ")
                ))
            })
    }

    /// What this family is served, in a phrase fit to follow "which is",
    /// for the seam that found a medium in the wrong drive.
    pub(crate) fn served_reading(self) -> String {
        let media: Vec<&str> = self.0.media.iter().map(|media| media.name).collect();
        match media.len() {
            0 => "a classifying name and is served nothing".to_owned(),
            _ => format!("served {}", media.join(" and ")),
        }
    }
}

impl PartialEq for DeviceFamily {
    fn eq(&self, other: &Self) -> bool {
        self.0.id == other.0.id
    }
}

impl Eq for DeviceFamily {}

impl std::hash::Hash for DeviceFamily {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.id.hash(state);
    }
}

impl fmt::Debug for DeviceFamily {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "DeviceFamily({})", self.0.id)
    }
}

impl fmt::Display for DeviceFamily {
    /// The family's name, which is what a refusal or a listing shows a
    /// user. The stable spelling is [`DeviceFamily::id`], deliberately
    /// not this.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0.name)
    }
}

static STORAGE_DEVICE: FamilyEntry = FamilyEntry {
    id: "storage-device",
    name: "storage device",
    kind_of: None,
    provenance: "the root of the lineage P32 states: the slot and mechanism side \
                 of the model, named by an attachment identity, holding zero or \
                 one medium",
    slot_prefix: None,
    media: &[],
    flux_path: None,
};

static FLOPPY_DRIVE: FamilyEntry = FamilyEntry {
    id: "floppy-drive",
    name: "floppy drive",
    kind_of: Some(&STORAGE_DEVICE),
    provenance: "the interior name for a drive taking flexible magnetic media: a \
                 removable article the drive's head touches, stepped to a \
                 location and read as the disk turns",
    slot_prefix: None,
    media: &[],
    flux_path: None,
};

static CBM_FLOPPY_DRIVE: FamilyEntry = FamilyEntry {
    id: "cbm-floppy-drive",
    name: "Commodore floppy drive",
    kind_of: Some(&FLOPPY_DRIVE),
    provenance: "the grouping the shared characteristics warrant: Commodore's \
                 drives carry their own processor and DOS, are addressed over \
                 the serial bus by unit number, and record in the group code \
                 the C1541 profile declares",
    slot_prefix: None,
    media: &[],
    flux_path: None,
};

static COMMODORE_1541: FamilyEntry = FamilyEntry {
    id: "commodore-1541",
    name: "Commodore 1541",
    kind_of: Some(&CBM_FLOPPY_DRIVE),
    provenance: "declared from the same published 1541 conventions the drive \
                 profile is: a single-surface drive served soft-sectored \
                 5.25-inch media, with a claimed physical recording path",
    slot_prefix: Some("cbmfloppy"),
    media: &[&FLEXIBLE_5_25_SOFT],
    flux_path: Some(&drive_profile::C1541),
};

static HEATHKIT_H17: FamilyEntry = FamilyEntry {
    id: "heathkit-h17",
    name: "Heathkit H-17 floppy drive",
    kind_of: Some(&FLOPPY_DRIVE),
    provenance: "declared from the published H17 conventions the H8D image \
                 format is written against: a single-surface drive served \
                 ten-sector hard-sectored 5.25-inch media, the medium's own \
                 holes dividing the revolution",
    slot_prefix: Some("heathfloppy"),
    media: &[&FLEXIBLE_5_25_HARD_10],
    flux_path: None,
};

static HARD_DISK: FamilyEntry = FamilyEntry {
    id: "hard-disk",
    name: "hard disk",
    kind_of: Some(&STORAGE_DEVICE),
    provenance: "D19's device-family name for the drive whose medium is \
                 logical-block media: geometry-opaque blocks addressed by \
                 number, with nothing beneath the block interface observable \
                 to a caller and therefore nothing more concrete to assert",
    slot_prefix: Some("hdd"),
    media: &[&LOGICAL_BLOCK_512],
    flux_path: None,
};

/// The enrolled families. Adding one changes its declaration, its tests,
/// and this list — the lineage is data, so nothing else is wired up.
static ARCHIVE_DEVICE: FamilyEntry = FamilyEntry {
    id: "archive-device",
    name: "archive slot",
    kind_of: Some(&STORAGE_DEVICE),
    provenance: "declared from what an archive is rather than from a drive that \
                 exists: a virtual medium is held by nothing, so its device is \
                 the receiver a load requires and the attachment identity its \
                 machine knows it by, and nothing more",
    slot_prefix: Some("arc"),
    media: &[&ARCHIVE],
    flux_path: None,
};

static ENROLLED: [&FamilyEntry; 7] = [
    &STORAGE_DEVICE,
    &FLOPPY_DRIVE,
    &CBM_FLOPPY_DRIVE,
    &COMMODORE_1541,
    &HEATHKIT_H17,
    &HARD_DISK,
    &ARCHIVE_DEVICE,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_entry_is_named_once_and_carries_its_provenance() {
        for family in DeviceFamily::enrolled() {
            assert!(!family.id().is_empty(), "an entry with no id names nothing");
            assert!(!family.name().is_empty(), "{} has no name", family.id());
            assert!(
                !family.provenance().is_empty(),
                "{} declares a machine fact from nowhere",
                family.id()
            );
            assert_eq!(
                DeviceFamily::from_id(family.id()).expect("an enrolled family resolves"),
                family
            );
        }

        let mut ids: Vec<&str> = DeviceFamily::enrolled().iter().map(|f| f.id()).collect();
        let count = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), count, "two entries share an id");
    }

    #[test]
    fn the_lineage_is_data_and_every_line_reaches_the_root() {
        // The amendment's own example, checked rather than described: a
        // Commodore 1541 is a CBM floppy drive, which is a floppy drive.
        assert_eq!(
            DeviceFamily::COMMODORE_1541.kind_of(),
            Some(DeviceFamily::CBM_FLOPPY_DRIVE)
        );
        assert!(DeviceFamily::COMMODORE_1541.is_a(DeviceFamily::FLOPPY_DRIVE));
        assert!(DeviceFamily::HEATHKIT_H17.is_a(DeviceFamily::FLOPPY_DRIVE));
        assert!(
            !DeviceFamily::HEATHKIT_H17.is_a(DeviceFamily::CBM_FLOPPY_DRIVE),
            "a Heathkit drive is not a Commodore one"
        );
        assert!(
            !DeviceFamily::HARD_DISK.is_a(DeviceFamily::FLOPPY_DRIVE),
            "a hard disk is no kind of floppy drive"
        );
        assert!(
            DeviceFamily::COMMODORE_1541.is_a(DeviceFamily::COMMODORE_1541),
            "a family is a kind of itself"
        );

        for family in DeviceFamily::enrolled() {
            let line = family.lineage();
            assert_eq!(
                *line.last().expect("a lineage holds at least its own entry"),
                DeviceFamily::STORAGE_DEVICE,
                "{} reaches no root",
                family.id()
            );
        }
        assert_eq!(DeviceFamily::STORAGE_DEVICE.kind_of(), None);
    }

    #[test]
    fn interior_names_classify_and_only_concrete_entries_instantiate() {
        for interior in [
            DeviceFamily::STORAGE_DEVICE,
            DeviceFamily::FLOPPY_DRIVE,
            DeviceFamily::CBM_FLOPPY_DRIVE,
        ] {
            assert!(!interior.is_concrete(), "{} instantiates", interior.id());
            assert_eq!(interior.slot_prefix(), None, "{} names a slot", interior.id());
            assert!(
                interior.accepted_media().is_empty(),
                "{} declares media a device could be checked against",
                interior.id()
            );
        }

        for concrete in DeviceFamily::concrete() {
            let prefix = concrete.slot_prefix().expect("a concrete entry names a slot");
            assert!(
                !prefix.chars().any(|c| c.is_ascii_digit()),
                "{prefix} carries a digit, and an attachment identity parses as \
                 prefix-then-index"
            );
            assert!(
                !concrete.accepted_media().is_empty(),
                "{} accepts nothing and could hold no medium",
                concrete.id()
            );
        }

        let mut prefixes: Vec<&str> = DeviceFamily::concrete()
            .iter()
            .filter_map(|family| family.slot_prefix())
            .collect();
        let count = prefixes.len();
        prefixes.sort_unstable();
        prefixes.dedup();
        assert_eq!(prefixes.len(), count, "two families take one slot prefix");
    }

    #[test]
    fn a_claimed_flux_path_is_a_family_declaration() {
        // P22: the family declares the recording path, and the drive
        // profile below it declares the rules. The two must agree about
        // the article — a family served one disk whose profile is
        // declared over another would be describing two drives.
        assert_eq!(DeviceFamily::COMMODORE_1541.flux_path(), Some("c1541"));
        assert!(
            DeviceFamily::COMMODORE_1541.accepts(&FLEXIBLE_5_25_SOFT),
            "the family accepts what its own profile is declared over"
        );
        assert_eq!(drive_profile::C1541.media.id, "flexible-5.25-soft");

        assert_eq!(
            DeviceFamily::HARD_DISK.flux_path(),
            None,
            "a logical-block device has no recording path to declare"
        );
        assert_eq!(DeviceFamily::HEATHKIT_H17.flux_path(), None);
    }

    #[test]
    fn an_unclaimed_family_is_refused_by_name() {
        // P3: the catalog is an enumerated claim. An optical drive is the
        // obvious next entry and naming it must refuse rather than
        // approximate it from a floppy declaration.
        let error = DeviceFamily::from_id("optical-drive").expect_err("refused");
        let message = error.to_string();
        assert!(
            message.contains("optical-drive"),
            "names what was asked: {message}"
        );
        assert!(
            message.contains("commodore-1541"),
            "names what is claimed: {message}"
        );

        let error = DeviceFamily::by_slot_prefix("cdrom").expect_err("refused");
        assert!(error.to_string().contains("hdd"), "names the claimed slots");
    }

    #[test]
    fn a_family_reads_out_what_it_is_served() {
        let reading = DeviceFamily::HEATHKIT_H17.served_reading();
        assert!(reading.starts_with("served "), "{reading}");
        assert!(reading.contains("hard-sectored"), "{reading}");
        assert!(
            DeviceFamily::FLOPPY_DRIVE
                .served_reading()
                .contains("classifying"),
            "an interior name is served nothing"
        );
    }
}
