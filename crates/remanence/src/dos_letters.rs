// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The DOS drive-letter composer: a namespace-mapping composer (P19).
//!
//! A DOS machine persists no drive-letter map. Its letters were assigned
//! at boot by a rule over the machine's own configuration — which media
//! occupied which slots, in which order the disks were attached — and
//! nothing on the disks records the result. There is no evidence to read,
//! so this seam *derives* the mapping from one named assignment rule and
//! the machine facts the caller asserts, and everything below exists to
//! keep that derivation from becoming a guess.
//!
//! The facts are the caller's, and they are read from the caller. A
//! session's device set holds only the block family, which cannot express
//! a floppy slot, a CD-ROM drive, or DOS attachment order; when those
//! families are claimed, this composer may take the same facts from a
//! session's devices instead of from an assertion. Nothing else about it
//! changes with that: it still opens no artifact, still names the rule it
//! applied, and still reports what the rule cannot settle as undetermined.
//!
//! Three constraints govern the derivation:
//!
//! - **The rule is an enumerated claim (P3).** The map names the rules
//!   applied to produce it, and a DOS variant outside
//!   [`DosAssignmentRule`] is refused by name rather than approximated by
//!   the nearest claimed one.
//! - **Evidence outranks a rule.** This form exists for a system that
//!   persists nothing. It is never a fallback for a persisted mapping
//!   that could not be read.
//! - **A derived mapping is not evidence.** The asserted facts and the
//!   applied rules travel with the answer as
//!   [`provenance`](DriveMap::provenance) — deliberately not called
//!   evidence — and whatever the rules cannot settle is
//!   [`Undetermined`](LetterOutcome::Undetermined) at the granularity of
//!   the letter it failed to establish, never filled in from position,
//!   size, order, label, or which volume happened to read cleanly.

use std::collections::BTreeMap;
use std::fmt;

use crate::error::{Error, Result};
use crate::report::{DiskReport, RegionId, RegionInfo, RegionRole, VolumeId, VolumeOrigin};

/// The partition types the claimed DOS variants letter. A type outside
/// this set takes no letter under any claimed rule — which is what
/// "hidden" means for `0x11`, `0x14` and `0x16`, and what a FAT32 or
/// LBA-addressed type means to a DOS that shipped before it existed.
const DOS_PARTITION_TYPES: [u8; 3] = [0x01, 0x04, 0x06];

/// The extended-container type the claimed variants follow. The
/// LBA-addressed container (`0x0f`) is a later type, so its chain's
/// logical drives take no letter under either claimed rule even though
/// this library reads them.
const DOS_EXTENDED_TYPE: u8 = 0x05;

/// The first letter a fixed disk's volume can take. `A:` and `B:` belong
/// to the floppy slots whether or not the machine has any.
const FIRST_FIXED_LETTER: char = 'C';

const LAST_LETTER: char = 'Z';

/// One named DOS drive-letter assignment rule (P3).
///
/// DOS did not letter volumes in the order a report lists them, and the
/// variants of DOS differ from each other in exactly one place: what
/// becomes of a second primary DOS partition on one disk. Each entry here
/// is a claim about the variants it names, not about every DOS that
/// shipped — DOS 2.x through 3.3 are outside the claim and refused by
/// name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DosAssignmentRule {
    /// MS-DOS 4.0 and 4.01: the first primary DOS partition of each disk
    /// in attachment order, then the logical drives of each disk's
    /// extended partition in the same order. A further primary DOS
    /// partition on a disk receives no letter at all.
    MsDos4,
    /// MS-DOS 5.0 through 6.22, and PC DOS of the same generation: as
    /// `MsDos4`, and then each remaining primary DOS partition, again by
    /// disk in attachment order.
    MsDos5,
}

impl DosAssignmentRule {
    /// Every rule this release claims, in the order a map lists them.
    pub const CLAIMED: [Self; 2] = [Self::MsDos4, Self::MsDos5];

    /// The rule's stable cross-language spelling.
    pub fn name(self) -> &'static str {
        match self {
            Self::MsDos4 => "ms-dos-4",
            Self::MsDos5 => "ms-dos-5",
        }
    }

    /// What the rule says, in a sentence fit to show a user beside the
    /// mapping it produced.
    pub fn reading(self) -> &'static str {
        match self {
            Self::MsDos4 => {
                "MS-DOS 4.0 and 4.01: the first primary DOS partition of each \
                 disk in attachment order, then the logical drives of each \
                 disk's extended partition in the same order; a further \
                 primary DOS partition receives no letter"
            }
            Self::MsDos5 => {
                "MS-DOS 5.0 through 6.22: the first primary DOS partition of \
                 each disk in attachment order, then the logical drives of \
                 each disk's extended partition in the same order, then each \
                 remaining primary DOS partition by disk in that order"
            }
        }
    }

    /// Resolves a rule name, refusing a variant this release does not
    /// claim by name (P3).
    ///
    /// Rust callers hold the enum and cannot express an unclaimed
    /// variant; this exists for the C and Python surfaces, where the rule
    /// arrives as text and the refusal has to happen at that boundary.
    pub fn from_name(name: &str) -> Result<Self> {
        Self::CLAIMED
            .into_iter()
            .find(|rule| rule.name() == name)
            .ok_or_else(|| {
                Error::unsupported(format!(
                    "no DOS drive-letter assignment rule named '{name}' is \
                     claimed; this release claims '{}' and '{}'",
                    Self::MsDos4.name(),
                    Self::MsDos5.name()
                ))
            })
    }

    /// Whether this variant letters a disk's primary DOS partitions past
    /// the first — the one place the claimed variants disagree.
    fn letters_remaining_primaries(self) -> bool {
        match self {
            Self::MsDos4 => false,
            Self::MsDos5 => true,
        }
    }
}

impl fmt::Display for DosAssignmentRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// One device in the machine the caller is describing.
///
/// This is caller-asserted machine configuration, not evidence: which
/// medium sits in which slot, and in what order the disks were attached,
/// are facts about the machine and appear in no image.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MachineDevice {
    /// A floppy drive, by slot. A PC has at most two, so only slots 0 and
    /// 1 are claimed.
    Floppy(u32),
    /// A fixed disk, by attachment order — 0 is the first attached.
    FixedDisk(u32),
    /// A CD-ROM drive, by attachment order. It takes a letter only where
    /// the caller declares where the resident driver placed it.
    CdRom(u32),
}

impl MachineDevice {
    /// The device kind's stable cross-language spelling.
    pub fn kind(self) -> &'static str {
        match self {
            Self::Floppy(_) => "floppy",
            Self::FixedDisk(_) => "fixed-disk",
            Self::CdRom(_) => "cd-rom",
        }
    }

    /// The slot or attachment order the caller asserted.
    pub fn index(self) -> u32 {
        match self {
            Self::Floppy(index) | Self::FixedDisk(index) | Self::CdRom(index) => index,
        }
    }
}

impl fmt::Display for MachineDevice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Floppy(slot) => write!(f, "floppy slot {slot}"),
            Self::FixedDisk(order) => write!(f, "fixed disk {order}"),
            Self::CdRom(order) => write!(f, "cd-rom drive {order}"),
        }
    }
}

/// A runtime condition of the machine that sits outside every claimed
/// rule.
///
/// Each of these could move or add letters at boot, and none of them is
/// modelled by any rule here. Declaring one does not make the composer
/// guess what it did: the letters it could have changed are reported
/// undetermined, which is the whole difference between this seam and a
/// consumer's own copy of the assignment order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResidentCondition {
    /// A `LASTDRIVE` ceiling the machine's `CONFIG.SYS` declared. Letters
    /// the rule assigns above it are undetermined; the claimed rules
    /// themselves assign without a ceiling.
    LastDrive(char),
    /// `SUBST` was in use.
    Subst,
    /// `JOIN` was in use.
    Join,
    /// `ASSIGN` was in use.
    Assign,
    /// A resident block-device driver was loaded.
    BlockDeviceDriver,
    /// A network redirector was loaded.
    NetworkRedirector,
}

impl ResidentCondition {
    /// The condition's stable cross-language spelling. `LASTDRIVE` spells
    /// its ceiling into the name, because the ceiling is the condition.
    pub fn name(self) -> String {
        match self {
            Self::LastDrive(letter) => format!("lastdrive={letter}"),
            Self::Subst => "subst".to_owned(),
            Self::Join => "join".to_owned(),
            Self::Assign => "assign".to_owned(),
            Self::BlockDeviceDriver => "block-device-driver".to_owned(),
            Self::NetworkRedirector => "network-redirector".to_owned(),
        }
    }

    /// Parses a condition — `lastdrive=E`, `subst`, `join`, `assign`,
    /// `block-device-driver`, `network-redirector` — refusing anything
    /// else by name. For the C and Python surfaces, where a condition
    /// arrives as text.
    pub fn parse(text: &str) -> Result<Self> {
        if let Some(ceiling) = text.strip_prefix("lastdrive=") {
            let mut letters = ceiling.chars();
            let (Some(letter), None) = (letters.next(), letters.next()) else {
                return Err(unclaimed_letter(ceiling));
            };
            return Ok(Self::LastDrive(drive_letter(letter)?));
        }
        match text {
            "subst" => Ok(Self::Subst),
            "join" => Ok(Self::Join),
            "assign" => Ok(Self::Assign),
            "block-device-driver" => Ok(Self::BlockDeviceDriver),
            "network-redirector" => Ok(Self::NetworkRedirector),
            other => Err(Error::unsupported(format!(
                "no machine condition named '{other}' is claimed; this \
                 release claims 'lastdrive=<letter>', 'subst', 'join', \
                 'assign', 'block-device-driver' and 'network-redirector'"
            ))),
        }
    }

    /// Why this condition leaves a letter unsettled, as the sentence an
    /// undetermined outcome carries.
    fn reading(self) -> String {
        match self {
            Self::LastDrive(letter) => format!(
                "the machine declared LASTDRIVE={letter}, and no claimed rule \
                 models a ceiling: the letter this rule assigns sits above it"
            ),
            Self::Subst => {
                "the machine ran SUBST, which no claimed rule models: any \
                 letter it redirected is not the one the rule assigns"
                    .to_owned()
            }
            Self::Join => {
                "the machine ran JOIN, which no claimed rule models: a joined \
                 drive is reachable as a directory of another and its own \
                 letter is not what the rule assigns"
                    .to_owned()
            }
            Self::Assign => {
                "the machine ran ASSIGN, which no claimed rule models: it \
                 redirects one letter to another wholesale"
                    .to_owned()
            }
            Self::BlockDeviceDriver => {
                "the machine loaded a resident block-device driver, which no \
                 claimed rule models: it adds or displaces letters at boot"
                    .to_owned()
            }
            Self::NetworkRedirector => {
                "the machine loaded a network redirector, which no claimed \
                 rule models: it claims letters from a source no image holds"
                    .to_owned()
            }
        }
    }

    /// Whether the condition puts every letter in doubt, as against
    /// bounding the range the rule may assign in.
    fn unbounds_every_letter(self) -> bool {
        !matches!(self, Self::LastDrive(_))
    }
}

/// What one drive letter turned out to name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LetterOutcome {
    /// The rule established the letter, and it names this volume on this
    /// device. The volume is named by the identity its own report issued
    /// — the value a caller passes back into a file verb.
    Volume {
        device: MachineDevice,
        volume: VolumeId,
    },
    /// The letter names a device the caller declared a resident driver
    /// placed here. The library composes no volume for such a device, so
    /// there is no identity to name and none is invented.
    DeclaredDevice { device: MachineDevice },
    /// DOS's phantom second floppy: on a single-floppy machine the second
    /// letter exists and names the same drive as `of`, prompting for a
    /// disk swap rather than naming a second volume.
    Phantom { of: char },
    /// The claimed rules could not settle this letter, and it is left
    /// unsettled rather than filled from something that merely looks
    /// right.
    Undetermined { reason: String },
}

impl LetterOutcome {
    /// The outcome's stable cross-language spelling.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Volume { .. } => "volume",
            Self::DeclaredDevice { .. } => "declared-device",
            Self::Phantom { .. } => "phantom",
            Self::Undetermined { .. } => "undetermined",
        }
    }
}

/// One letter and what it names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriveMapping {
    /// The letter itself, `A` through `Z`, without its colon.
    pub letter: char,
    pub outcome: LetterOutcome,
}

/// The mapping a rule established over asserted machine facts.
///
/// A letter absent from [`mappings`](Self::mappings) is a letter the
/// machine had no drive at: it is different from a letter that exists and
/// could not be settled, which is present and
/// [`Undetermined`](LetterOutcome::Undetermined).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriveMap {
    /// The rules applied — one where the caller stated the variant, and
    /// every claimed rule where it did not.
    pub applied_rules: Vec<DosAssignmentRule>,
    /// Every letter the machine had a drive at, in letter order.
    pub mappings: Vec<DriveMapping>,
    /// The asserted facts and the applied rules, travelling with the
    /// answer. **This is not evidence**: nothing here was read off a
    /// disk, and calling it evidence would put a derivation beside the
    /// observations that identification carries (P4).
    pub provenance: Vec<String>,
}

impl DriveMap {
    /// What this letter names, or `None` where the machine had no drive
    /// at it.
    pub fn letter(&self, letter: char) -> Option<&DriveMapping> {
        self.mappings.iter().find(|mapping| mapping.letter == letter)
    }

    /// How many letters the rules established — the count that excludes
    /// every undetermined one.
    pub fn established_count(&self) -> usize {
        self.mappings
            .iter()
            .filter(|mapping| !matches!(mapping.outcome, LetterOutcome::Undetermined { .. }))
            .count()
    }
}

/// The machine facts a caller asserts, and the composer that maps letters
/// over them.
///
/// The composer opens no artifact: it takes the inspection reports the
/// caller already holds, and every report stays the caller's. It composes
/// no file container over the result either — the letter is what a
/// consumer shows a user, and the volume identity is what it passes back
/// into a file verb.
#[derive(Debug, Default)]
pub struct DosMachine<'a> {
    floppies: Vec<(u32, &'a DiskReport)>,
    fixed_disks: Vec<(u32, &'a DiskReport)>,
    cd_roms: Vec<(u32, Option<char>)>,
    conditions: Vec<ResidentCondition>,
}

impl<'a> DosMachine<'a> {
    /// A machine with nothing asserted about it yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Asserts that the medium `report` inspects occupies floppy slot
    /// `slot`. Slot 0 is `A:`; a PC has at most two floppy drives, so a
    /// slot above 1 is refused by name.
    pub fn assert_floppy(&mut self, slot: u32, report: &'a DiskReport) -> Result<()> {
        if slot > 1 {
            return Err(Error::unsupported(format!(
                "floppy slot {slot} is outside the claim; DOS letters two \
                 floppy slots, 0 as A: and 1 as B:"
            )));
        }
        if self.floppies.iter().any(|(taken, _)| *taken == slot) {
            return Err(occupied(MachineDevice::Floppy(slot)));
        }
        self.floppies.push((slot, report));
        self.floppies.sort_by_key(|(slot, _)| *slot);
        Ok(())
    }

    /// Asserts that the medium `report` inspects is the fixed disk
    /// attached at `order` — 0 being the first attached, which is the
    /// order DOS assigned letters in.
    pub fn assert_fixed_disk(&mut self, order: u32, report: &'a DiskReport) -> Result<()> {
        if self.fixed_disks.iter().any(|(taken, _)| *taken == order) {
            return Err(occupied(MachineDevice::FixedDisk(order)));
        }
        self.fixed_disks.push((order, report));
        self.fixed_disks.sort_by_key(|(order, _)| *order);
        Ok(())
    }

    /// Asserts a CD-ROM drive at attachment order `order`.
    ///
    /// `driver_letter` is where the caller declares the resident driver
    /// placed it. Nothing on the disks records that placement and the
    /// driver could put it anywhere, so an undeclared CD-ROM takes no
    /// letter rather than a guessed one.
    pub fn assert_cdrom(&mut self, order: u32, driver_letter: Option<char>) -> Result<()> {
        if self.cd_roms.iter().any(|(taken, _)| *taken == order) {
            return Err(occupied(MachineDevice::CdRom(order)));
        }
        let driver_letter = driver_letter.map(drive_letter).transpose()?;
        if let Some(letter) = driver_letter {
            if let Some((other, _)) = self
                .cd_roms
                .iter()
                .find(|(_, taken)| *taken == Some(letter))
            {
                return Err(Error::unsupported(format!(
                    "{letter}: is already declared for {}; two drives cannot \
                     be declared at one letter",
                    MachineDevice::CdRom(*other)
                )));
            }
        }
        self.cd_roms.push((order, driver_letter));
        self.cd_roms.sort_by_key(|(order, _)| *order);
        Ok(())
    }

    /// Declares a runtime condition of the machine that sits outside
    /// every claimed rule. The letters it could have changed come back
    /// undetermined.
    pub fn declare(&mut self, condition: ResidentCondition) {
        if !self.conditions.contains(&condition) {
            self.conditions.push(condition);
        }
    }

    /// Composes the drive-letter mapping.
    ///
    /// `rule` is the variant the caller states the machine ran. Where it
    /// states none, every claimed rule is applied and a letter the rules
    /// disagree on comes back undetermined rather than settled by
    /// choosing the most common one.
    pub fn compose(&self, rule: Option<DosAssignmentRule>) -> Result<DriveMap> {
        let applied_rules: Vec<DosAssignmentRule> = match rule {
            Some(rule) => vec![rule],
            None => DosAssignmentRule::CLAIMED.to_vec(),
        };

        let mut provenance = vec![
            "this mapping is derived from an assignment rule applied to \
             caller-asserted machine facts; it is provenance, not evidence \
             read off a disk"
                .to_owned(),
        ];
        if rule.is_none() {
            provenance.push(
                "the caller stated no DOS variant, so every claimed rule was \
                 applied and a letter they disagree on is undetermined"
                    .to_owned(),
            );
        }
        for applied in &applied_rules {
            provenance.push(format!("rule {}: {}", applied.name(), applied.reading()));
        }

        // A machine whose only floppy sits in the second slot is not a
        // machine: DOS's first floppy is slot 0, which is exactly why a
        // single-floppy machine's second letter is the phantom of A:.
        if self.floppies.iter().any(|(slot, _)| *slot == 1)
            && !self.floppies.iter().any(|(slot, _)| *slot == 0)
        {
            return Err(Error::unsupported(
                "floppy slot 1 was asserted without slot 0; DOS's first \
                 floppy drive is slot 0, and a machine whose only floppy is \
                 the second drive has no assignment rule to apply",
            ));
        }

        let mut letters: BTreeMap<char, LetterOutcome> = BTreeMap::new();
        self.map_floppies(&mut letters, &mut provenance);

        let claimed: Vec<Vec<Claim>> = applied_rules
            .iter()
            .map(|applied| self.fixed_disk_claims(*applied))
            .collect();
        self.describe_fixed_disks(&mut provenance);
        merge_fixed_claims(&applied_rules, &claimed, &mut letters, &mut provenance);

        self.map_cd_roms(&mut letters, &mut provenance)?;
        self.apply_conditions(&mut letters, &mut provenance);

        Ok(DriveMap {
            applied_rules,
            mappings: letters
                .into_iter()
                .map(|(letter, outcome)| DriveMapping { letter, outcome })
                .collect(),
            provenance,
        })
    }

    /// `A:` and `B:`, which every claimed rule agrees on. A machine with
    /// no floppy drive has neither letter at all, and a machine with one
    /// has `B:` as the phantom of `A:`.
    fn map_floppies(&self, letters: &mut BTreeMap<char, LetterOutcome>, provenance: &mut Vec<String>) {
        for (slot, report) in &self.floppies {
            let device = MachineDevice::Floppy(*slot);
            let letter = if *slot == 0 { 'A' } else { 'B' };
            match whole_device_volume(report) {
                Some(volume) => {
                    letters.insert(letter, LetterOutcome::Volume { device, volume });
                    provenance.push(format!("{letter}: is {device}, as asserted"));
                }
                None => {
                    letters.insert(
                        letter,
                        LetterOutcome::Undetermined {
                            reason: format!(
                                "{device} holds a medium no volume composed from, \
                                 so the letter DOS gave the drive names nothing \
                                 this library can hand back"
                            ),
                        },
                    );
                    provenance.push(format!(
                        "{letter}: is {device}, whose medium composed no \
                         whole-device volume"
                    ));
                }
            }
        }

        if self.floppies.len() == 1 && self.floppies[0].0 == 0 {
            letters.insert('B', LetterOutcome::Phantom { of: 'A' });
            provenance.push(
                "B: is the phantom drive: a single-floppy machine still has \
                 two letters, and the second is the same drive prompting for \
                 a disk swap"
                    .to_owned(),
            );
        }
    }

    /// What each asserted fixed disk contributes, said once rather than
    /// once per applied rule.
    fn describe_fixed_disks(&self, provenance: &mut Vec<String>) {
        for (order, report) in &self.fixed_disks {
            let device = MachineDevice::FixedDisk(*order);
            if report.partition_schema.is_none() {
                provenance.push(format!(
                    "{device} declares no partition schema ({}), and no claimed \
                     rule letters a fixed disk without one",
                    report.content.name()
                ));
                continue;
            }
            let primaries = dos_primaries(report).count();
            let logicals = dos_logicals(report).count();
            provenance.push(format!(
                "{device} declares {primaries} primary and {logicals} logical \
                 DOS partition(s) a claimed rule letters"
            ));
            if logicals > 0 && !follows_extended_chain(report) {
                provenance.push(format!(
                    "{device}'s extended container is not type 0x{DOS_EXTENDED_TYPE:02x}, \
                     which no claimed variant follows, so its logical drives \
                     take no letter"
                ));
            }
        }
    }

    /// The letters one rule assigns to the fixed disks, in the order it
    /// assigns them: first primaries, then logicals, then — where the
    /// variant does so at all — the remaining primaries.
    fn fixed_disk_claims(&self, rule: DosAssignmentRule) -> Vec<Claim> {
        let mut claims = Vec::new();

        for (order, report) in &self.fixed_disks {
            if let Some(region) = dos_primaries(report).next() {
                claims.push(Claim::of(
                    MachineDevice::FixedDisk(*order),
                    report,
                    region,
                    "the first primary DOS partition",
                ));
            }
        }

        for (order, report) in &self.fixed_disks {
            if !follows_extended_chain(report) {
                continue;
            }
            for region in dos_logicals(report) {
                claims.push(Claim::of(
                    MachineDevice::FixedDisk(*order),
                    report,
                    region,
                    "a logical drive of the extended partition",
                ));
            }
        }

        if rule.letters_remaining_primaries() {
            for (order, report) in &self.fixed_disks {
                for region in dos_primaries(report).skip(1) {
                    claims.push(Claim::of(
                        MachineDevice::FixedDisk(*order),
                        report,
                        region,
                        "a further primary DOS partition",
                    ));
                }
            }
        }

        claims
    }

    /// The letters the caller declared a resident CD-ROM driver placed.
    fn map_cd_roms(
        &self,
        letters: &mut BTreeMap<char, LetterOutcome>,
        provenance: &mut Vec<String>,
    ) -> Result<()> {
        for (order, declared) in &self.cd_roms {
            let device = MachineDevice::CdRom(*order);
            let Some(letter) = declared else {
                provenance.push(format!(
                    "{device} takes no letter: nothing on the disks records \
                     where its driver placed it, and the caller declared no \
                     placement"
                ));
                continue;
            };
            if letters.contains_key(letter) {
                return Err(Error::unsupported(format!(
                    "{letter}: was declared for {device}, and the applied rule \
                     assigns that letter as well; the asserted facts \
                     contradict each other"
                )));
            }
            letters.insert(*letter, LetterOutcome::DeclaredDevice { device });
            provenance.push(format!(
                "{letter}: is {device}, where the caller declares its resident \
                 driver placed it"
            ));
        }
        Ok(())
    }

    /// Turns every letter a declared condition could have changed
    /// undetermined.
    fn apply_conditions(
        &self,
        letters: &mut BTreeMap<char, LetterOutcome>,
        provenance: &mut Vec<String>,
    ) {
        if self.conditions.is_empty() {
            provenance.push(
                "no LASTDRIVE ceiling, SUBST, JOIN, ASSIGN, resident \
                 block-device driver or network redirector was declared; the \
                 claimed rules assign without any of them"
                    .to_owned(),
            );
            return;
        }

        for condition in &self.conditions {
            provenance.push(format!(
                "the machine declared {}, which is outside every claimed rule",
                condition.name()
            ));
            for (letter, outcome) in letters.iter_mut() {
                let unsettled = match condition {
                    ResidentCondition::LastDrive(ceiling) => letter > ceiling,
                    other => other.unbounds_every_letter(),
                };
                if unsettled && !matches!(outcome, LetterOutcome::Undetermined { .. }) {
                    *outcome = LetterOutcome::Undetermined {
                        reason: condition.reading(),
                    };
                }
            }
        }
    }
}

/// One rule's claim on one letter, before the claims of the other applied
/// rules are set beside it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Claim {
    device: MachineDevice,
    volume: Option<VolumeId>,
    /// What the rule matched, as the phrase a reason or a provenance line
    /// quotes.
    reading: String,
}

impl Claim {
    fn of(device: MachineDevice, report: &DiskReport, region: &RegionInfo, clause: &str) -> Self {
        Self {
            device,
            volume: volume_on_region(report, region.id),
            reading: format!(
                "{clause} of {device} (region {}, type 0x{:02x})",
                region.declared_number, region.declared_type
            ),
        }
    }
}

/// Sets the applied rules' claims side by side and turns each letter into
/// an outcome: what they agree on is established, and what they disagree
/// on is undetermined with both readings in the reason.
fn merge_fixed_claims(
    rules: &[DosAssignmentRule],
    claimed: &[Vec<Claim>],
    letters: &mut BTreeMap<char, LetterOutcome>,
    provenance: &mut Vec<String>,
) {
    let widest = claimed.iter().map(Vec::len).max().unwrap_or(0);
    for position in 0..widest {
        let Some(letter) = fixed_letter(position) else {
            provenance.push(format!(
                "the applied rule assigns more volumes than there are letters; \
                 {} past {LAST_LETTER}: took none",
                widest - position
            ));
            break;
        };

        let answers: Vec<Option<&Claim>> = claimed.iter().map(|claims| claims.get(position)).collect();
        let first = answers[0];
        if answers.iter().all(|answer| *answer == first) {
            let claim = first.expect("a position inside the widest rule's claims");
            letters.insert(letter, established(claim));
            provenance.push(format!("{letter}: is {}", claim.reading));
            continue;
        }

        let readings: Vec<String> = rules
            .iter()
            .zip(&answers)
            .map(|(rule, answer)| match answer {
                Some(claim) => format!("{} assigns it to {}", rule.name(), claim.reading),
                None => format!("{} assigns no letter here", rule.name()),
            })
            .collect();
        letters.insert(
            letter,
            LetterOutcome::Undetermined {
                reason: format!(
                    "the claimed rules disagree: {}. State the variant the \
                     machine ran to settle it",
                    readings.join(", and ")
                ),
            },
        );
        provenance.push(format!("{letter}: the claimed rules disagree"));
    }
}

/// What an agreed claim establishes: the volume it names, or the reason
/// there is no identity to name.
fn established(claim: &Claim) -> LetterOutcome {
    match claim.volume {
        Some(volume) => LetterOutcome::Volume {
            device: claim.device,
            volume,
        },
        None => LetterOutcome::Undetermined {
            reason: format!(
                "the rule assigns this letter to {}, and no volume composed \
                 from it, so there is no identity to name; the letter still \
                 belongs to that partition and the letters behind it are \
                 unaffected",
                claim.reading
            ),
        },
    }
}

/// The letter the `position`-th fixed-disk volume takes, or `None` past
/// `Z:`.
fn fixed_letter(position: usize) -> Option<char> {
    let letter = u32::from(FIRST_FIXED_LETTER) + u32::try_from(position).ok()?;
    if letter > u32::from(LAST_LETTER) {
        return None;
    }
    char::from_u32(letter)
}

/// The primary DOS partitions of one disk, in the schema's own declared
/// order.
fn dos_primaries(report: &DiskReport) -> impl Iterator<Item = &RegionInfo> {
    lettered_regions(report, "primary")
}

/// The logical DOS partitions of one disk, in chain order.
fn dos_logicals(report: &DiskReport) -> impl Iterator<Item = &RegionInfo> {
    lettered_regions(report, "logical")
}

fn lettered_regions<'a>(
    report: &'a DiskReport,
    placement: &'a str,
) -> impl Iterator<Item = &'a RegionInfo> {
    report
        .regions
        .iter()
        .filter(move |region| {
            region.declared_placement == placement
                && region.role == RegionRole::Data
                && DOS_PARTITION_TYPES.contains(&region.declared_type)
        })
}

/// Whether this disk's extended container is one the claimed variants
/// follow. A container the library reads and DOS did not is not a chain
/// DOS lettered.
fn follows_extended_chain(report: &DiskReport) -> bool {
    report
        .regions
        .iter()
        .any(|region| region.role == RegionRole::Container && region.declared_type == DOS_EXTENDED_TYPE)
}

fn volume_on_region(report: &DiskReport, region: RegionId) -> Option<VolumeId> {
    report
        .volumes
        .iter()
        .find(|volume| match &volume.origin {
            VolumeOrigin::Regions(regions) => regions.contains(&region),
            VolumeOrigin::WholeDevice => false,
        })
        .map(|volume| volume.id)
}

fn whole_device_volume(report: &DiskReport) -> Option<VolumeId> {
    report
        .volumes
        .iter()
        .find(|volume| volume.origin == VolumeOrigin::WholeDevice)
        .map(|volume| volume.id)
}

/// Normalizes a caller's letter the way the DOS name seam normalizes a
/// caller's file name: an ASCII letter is stored as DOS stored it, in
/// upper case, and anything else is refused rather than repaired.
fn drive_letter(letter: char) -> Result<char> {
    letter
        .is_ascii_alphabetic()
        .then(|| letter.to_ascii_uppercase())
        .ok_or_else(|| unclaimed_letter(&letter.to_string()))
}

fn unclaimed_letter(text: &str) -> Error {
    Error::unsupported(format!(
        "'{text}' is not a drive letter; one is a single letter A through Z"
    ))
}

fn occupied(device: MachineDevice) -> Error {
    Error::unsupported(format!(
        "{device} was already asserted; a machine fact is stated once"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_claimed_rule_spells_itself_stably_and_reads_back() {
        for rule in DosAssignmentRule::CLAIMED {
            assert_eq!(
                DosAssignmentRule::from_name(rule.name()).expect("a claimed rule reads back"),
                rule
            );
            assert!(!rule.reading().is_empty(), "{rule} says what it does");
        }
        assert_eq!(DosAssignmentRule::MsDos4.name(), "ms-dos-4");
        assert_eq!(DosAssignmentRule::MsDos5.name(), "ms-dos-5");
    }

    /// P3: the variant set is an enumerated claim, and a DOS this release
    /// does not claim is refused by name rather than served by the
    /// nearest rule.
    #[test]
    fn an_unclaimed_variant_is_refused_by_name() {
        let error = DosAssignmentRule::from_name("ms-dos-3.3").expect_err("refused");
        let message = error.to_string();
        assert!(message.contains("ms-dos-3.3"), "names what was asked: {message}");
        assert!(message.contains("ms-dos-4"), "names what is claimed: {message}");
        assert!(message.contains("ms-dos-5"), "names what is claimed: {message}");
    }

    #[test]
    fn a_condition_round_trips_through_its_spelling() {
        for condition in [
            ResidentCondition::LastDrive('E'),
            ResidentCondition::Subst,
            ResidentCondition::Join,
            ResidentCondition::Assign,
            ResidentCondition::BlockDeviceDriver,
            ResidentCondition::NetworkRedirector,
        ] {
            assert_eq!(
                ResidentCondition::parse(&condition.name()).expect("parses"),
                condition
            );
        }
        assert_eq!(
            ResidentCondition::parse("lastdrive=e").expect("parses"),
            ResidentCondition::LastDrive('E'),
            "a letter is normalized as the DOS name seam normalizes a name"
        );
        assert!(ResidentCondition::parse("dblspace").is_err(), "unclaimed");
        assert!(ResidentCondition::parse("lastdrive=").is_err(), "no ceiling");
        assert!(ResidentCondition::parse("lastdrive=4").is_err(), "not a letter");
    }

    #[test]
    fn fixed_disk_letters_start_at_c_and_stop_at_z() {
        assert_eq!(fixed_letter(0), Some('C'));
        assert_eq!(fixed_letter(1), Some('D'));
        assert_eq!(fixed_letter(23), Some('Z'));
        assert_eq!(fixed_letter(24), None, "there is no letter past Z:");
    }

    #[test]
    fn a_machine_device_spells_its_kind_and_index() {
        assert_eq!(MachineDevice::Floppy(1).kind(), "floppy");
        assert_eq!(MachineDevice::FixedDisk(2).kind(), "fixed-disk");
        assert_eq!(MachineDevice::CdRom(0).kind(), "cd-rom");
        assert_eq!(MachineDevice::FixedDisk(2).index(), 2);
        assert_eq!(MachineDevice::Floppy(0).to_string(), "floppy slot 0");
    }
}
