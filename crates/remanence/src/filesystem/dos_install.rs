// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! DOS installation recognition: what a volume's own contents say about
//! the DOS installed on it.
//!
//! DOS persists its configuration, and this is the seam that reads it.
//! The kernel files in a root directory say which DOS this is; the FAT
//! boot record's OEM field says what formatted the volume; `CONFIG.SYS`
//! and `AUTOEXEC.BAT` declare what the machine loaded at boot. Every one
//! of them is a file or a field on a FAT volume this library already
//! reads, so none of them is a fact a caller should have to assert.
//!
//! **Recognition is an enumerated claim (P3).** The kernel sets this
//! release recognizes are named in [`DosKernel`], the DOS a claimed
//! assignment rule covers is narrower still, and everything outside
//! either is refused by name through [`InstallRule`] rather than served
//! by the nearest claimed answer. Recognizing a DOS and claiming an
//! assignment rule for it are **different acts**: FreeDOS is recognized
//! exactly and lettered by nothing, which is a named refusal rather than
//! a silent miss.
//!
//! **The version is read the way geometry is read.** Several sources
//! speak about it, each is kept with where it was taken, and what they
//! settle between them is the answer — [`DosVersion::Undetermined`]
//! where two of them disagree, which is a different fact from
//! [`DosVersion::Unstated`], where nothing spoke at all. Nothing here
//! prefers one reading by being listed first: a disagreement stands.

use std::fmt;

use crate::error::{Error, Result, RuleIdentity};
use crate::filesystem::dos_letters::{DosAssignmentRule, ResidentCondition};
use crate::model::media::Medium;
use crate::model::report::VolumeId;

/// The rules a DOS installation recognition refuses by name (P3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum InstallRule {
    /// No kernel file set this release recognizes sits in the volume's
    /// root directory, so nothing says a DOS is installed here.
    NoKernel,
    /// The version sources disagree, and no claimed rule follows from a
    /// version that was never settled.
    VersionUndetermined,
    /// The version is settled and sits outside every claimed rule — DOS
    /// 2.x and 3.x are the cases this release reaches.
    VersionNotClaimed,
}

impl InstallRule {
    /// Every rule in the set.
    pub const ALL: [Self; 3] = [
        Self::NoKernel,
        Self::VersionUndetermined,
        Self::VersionNotClaimed,
    ];

    /// The stable cross-language spelling a refusal carries.
    pub const fn as_str(self) -> RuleIdentity {
        match self {
            Self::NoKernel => "no-dos-kernel",
            Self::VersionUndetermined => "dos-version-undetermined",
            Self::VersionNotClaimed => "dos-version-not-claimed",
        }
    }
}

impl fmt::Display for InstallRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A kernel file set this release recognizes in a root directory.
///
/// The set is the evidence, not any one file: `IO.SYS` alone is a file
/// named `IO.SYS`, and `IO.SYS` beside `MSDOS.SYS` is MS-DOS. Each entry
/// names the files it requires, so what recognized an installation can be
/// quoted back exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DosKernel {
    /// MS-DOS: `IO.SYS` and `MSDOS.SYS`.
    MsDos,
    /// PC DOS and IBM DOS: `IBMBIO.COM` and `IBMDOS.COM`.
    PcDos,
    /// FreeDOS: `KERNEL.SYS`.
    ///
    /// Its letter order is not a property of its version — FreeDOS 1.x
    /// letters the way MS-DOS 5 and 6 do, by documented default, and its
    /// kernel takes a `DLASORT`/`DLA` setting that changes the order
    /// instead. So the rule follows the kernel here and the version says
    /// nothing about it, which is why the two are asked separately.
    FreeDos,
}

impl DosKernel {
    /// Every kernel set this release recognizes.
    pub const CLAIMED: [Self; 3] = [Self::MsDos, Self::PcDos, Self::FreeDos];

    /// The kernel set's stable cross-language spelling.
    pub const fn name(self) -> &'static str {
        match self {
            Self::MsDos => "ms-dos",
            Self::PcDos => "pc-dos",
            Self::FreeDos => "freedos",
        }
    }

    /// The files whose presence together recognizes this set, in the
    /// order a root directory is asked for them.
    pub const fn files(self) -> &'static [&'static str] {
        match self {
            Self::MsDos => &["IO.SYS", "MSDOS.SYS"],
            Self::PcDos => &["IBMBIO.COM", "IBMDOS.COM"],
            Self::FreeDos => &["KERNEL.SYS"],
        }
    }

    /// The startup configuration files this DOS reads, in the order it
    /// prefers them: the first one present is the one that governs, and
    /// the rest are not read.
    ///
    /// FreeDOS reads `FDCONFIG.SYS` ahead of `CONFIG.SYS` so it can sit
    /// beside another DOS on one volume without either reading the
    /// other's declarations. Reading both and merging them would invent
    /// a machine neither file describes.
    pub const fn config_files(self) -> &'static [&'static str] {
        match self {
            Self::MsDos | Self::PcDos => &["CONFIG.SYS"],
            Self::FreeDos => &["FDCONFIG.SYS", "CONFIG.SYS"],
        }
    }

    /// The startup batch files this DOS reads, in the order it prefers
    /// them, under the same first-one-present rule as
    /// [`config_files`](Self::config_files).
    pub const fn autoexec_files(self) -> &'static [&'static str] {
        match self {
            Self::MsDos | Self::PcDos => &["AUTOEXEC.BAT"],
            Self::FreeDos => &["FDAUTO.BAT", "AUTOEXEC.BAT"],
        }
    }

    /// The OEM-field prefixes whose trailing digits are a version of
    /// *this DOS*, used only to corroborate — never to recognize, an OEM
    /// field being a fact about what formatted the volume rather than
    /// about what boots it.
    ///
    /// `MSWIN` is deliberately absent. A volume Windows 95 formatted
    /// reads `MSWIN4.1`, whose `4.1` is a Windows version and not MS-DOS
    /// 4.01; taking it as a DOS reading would letter the machine by the
    /// MS-DOS 4 rule on the strength of a string that never said so.
    const fn oem_prefixes(self) -> &'static [&'static str] {
        match self {
            Self::MsDos => &["MSDOS"],
            Self::PcDos => &["IBM"],
            Self::FreeDos => &["FRDOS", "FREEDOS"],
        }
    }
}

impl fmt::Display for DosKernel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Where one reading of the DOS version was taken.
///
/// The sources are enumerated so a reading can never arrive without its
/// basis, and so two of them disagreeing is expressible rather than
/// silently resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum VersionSource {
    /// The FAT boot record's OEM name field, as `FORMAT` wrote it —
    /// `MSDOS5.0`, `IBM  4.0`, `FRDOS4.1`. It states what formatted the
    /// volume, which is evidence about the DOS and not proof of it.
    OemName,
    /// A version banner inside `COMMAND.COM`, where a claimed pattern
    /// reads one.
    CommandBanner,
}

impl VersionSource {
    /// The source's stable cross-language spelling.
    pub const fn name(self) -> &'static str {
        match self {
            Self::OemName => "oem-name",
            Self::CommandBanner => "command-banner",
        }
    }
}

impl fmt::Display for VersionSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// One source's reading of the DOS version, kept with where it was taken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionReading {
    pub source: VersionSource,
    pub major: u8,
    pub minor: u8,
    /// The bytes the reading was taken from, as read.
    pub stated: String,
}

impl VersionReading {
    fn new(source: VersionSource, major: u8, minor: u8, stated: impl Into<String>) -> Self {
        Self {
            source,
            major,
            minor,
            stated: stated.into(),
        }
    }
}

/// What the version sources settled between them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DosVersion {
    /// Every source that spoke agrees, and this is what they say.
    Settled { major: u8, minor: u8 },
    /// Two sources disagree. Both readings stand in
    /// [`DosInstallation::version_readings`] and neither is preferred.
    Undetermined,
    /// No source spoke at all, which is a different fact from two that
    /// disagree.
    Unstated,
}

impl DosVersion {
    /// The stable cross-language spelling of this outcome.
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Settled { .. } => "settled",
            Self::Undetermined => "undetermined",
            Self::Unstated => "unstated",
        }
    }

    /// The assignment rule this version implies, or the rule that refuses
    /// it by name.
    ///
    /// The claimed span is MS-DOS 4.0–4.01 and 5.0–6.22; DOS 2.x and 3.x
    /// are outside it and refused rather than served by `MsDos4`.
    pub fn assignment_rule(&self) -> Result<DosAssignmentRule> {
        match self {
            Self::Settled { major: 4, .. } => Ok(DosAssignmentRule::MsDos4),
            Self::Settled { major: 5 | 6, .. } => Ok(DosAssignmentRule::MsDos5),
            Self::Settled { major, minor } => Err(refusal(
                InstallRule::VersionNotClaimed,
                format!(
                    "the installed DOS reads as version {major}.{minor:02}, which no \
                     claimed assignment rule covers; this release claims MS-DOS 4.0 \
                     through 6.22"
                ),
            )),
            Self::Undetermined => Err(refusal(
                InstallRule::VersionUndetermined,
                "the version sources disagree, so no claimed assignment rule \
                 follows; both readings stand as evidence",
            )),
            Self::Unstated => Err(refusal(
                InstallRule::VersionUndetermined,
                "no source on this volume states a DOS version, so no claimed \
                 assignment rule follows",
            )),
        }
    }
}

/// What recognition established about the DOS installed on one volume.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DosInstallation {
    /// The volume the installation was recognized on.
    pub volume: VolumeId,
    /// The kernel set found in its root directory.
    pub kernel: DosKernel,
    /// What the version sources settled.
    pub version: DosVersion,
    /// Every reading taken, in the order the sources were consulted —
    /// present whether or not they agreed.
    pub version_readings: Vec<VersionReading>,
    /// The conditions `CONFIG.SYS` and `AUTOEXEC.BAT` declared, each of
    /// which unsettles letters no claimed rule models.
    pub conditions: Vec<ResidentCondition>,
    /// Whether the volume sits on a region the partition table flags
    /// active. That flag is what the boot sector follows, so it is what
    /// settles which of two installations on one disk actually booted.
    pub on_active_region: bool,
    /// The letter `MSCDEX` was told to place a CD-ROM at, where the
    /// startup files say so — its `/L:` switch, which is the one place a
    /// machine records that placement. Where nothing says, an optical
    /// drive takes no letter rather than a guessed one.
    pub optical_letter: Option<char>,
    /// What recognized this installation (P4).
    pub evidence: Vec<String>,
}

impl DosInstallation {
    /// The assignment rule this installation's own contents imply, or the
    /// refusal that names why none does.
    ///
    /// Which fact settles it depends on the DOS. MS-DOS and PC DOS
    /// changed their order between versions, so the version settles it
    /// and a version outside the claim is refused. FreeDOS did not: every
    /// release letters as MS-DOS 5 does unless its kernel was told
    /// otherwise, so the kernel settles it and the alternate order
    /// arrives as a declared condition rather than as a second rule.
    pub fn assignment_rule(&self) -> Result<DosAssignmentRule> {
        match self.kernel {
            DosKernel::FreeDos => Ok(DosAssignmentRule::MsDos5),
            DosKernel::MsDos | DosKernel::PcDos => self.version.assignment_rule(),
        }
    }
}

/// Recognizes the DOS installed on the volume at `offset`, or answers the
/// honest absence that no kernel set sits in its root directory.
///
/// `Ok(None)` is that absence and is not a failure: a volume with no DOS
/// on it is a fact about the volume. A refusal here is reserved for a
/// reading that broke.
pub(crate) fn recognize(
    medium: &mut Medium,
    volume: VolumeId,
    offset: u64,
    on_active_region: bool,
) -> Result<Option<DosInstallation>> {
    let Some(kernel) = recognize_kernel(medium, offset)? else {
        return Ok(None);
    };

    let mut evidence = vec![format!(
        "the root directory holds {}, which is {}'s kernel set",
        kernel.files().join(" and "),
        kernel
    )];

    let mut version_readings = Vec::new();
    if let Some(reading) = read_oem_version(medium, offset, kernel)? {
        evidence.push(format!(
            "the boot record's OEM field reads '{}'",
            reading.stated
        ));
        version_readings.push(reading);
    }
    if let Some(reading) = read_command_banner(medium, offset)? {
        evidence.push(format!(
            "COMMAND.COM carries the version banner '{}'",
            reading.stated
        ));
        version_readings.push(reading);
    }

    let version = settle(&version_readings);
    let (conditions, optical_letter) = read_conditions(medium, offset, kernel, &mut evidence)?;

    // Said out loud rather than left as a silence. FreeDOS's letter order
    // is a byte inside KERNEL.SYS, patched by `SYS CONFIG` and readable
    // from no configuration file; this release does not read it, so a
    // kernel patched to the per-drive order would letter differently than
    // reported. The default is the order applied here.
    if kernel == DosKernel::FreeDos {
        evidence.push(
            "FreeDOS stores its letter order (DLASORT) inside KERNEL.SYS rather \
             than in a configuration file, and this release does not read it; \
             the letters follow the kernel's documented default, which is the \
             MS-DOS order"
                .to_owned(),
        );
    }

    Ok(Some(DosInstallation {
        volume,
        kernel,
        version,
        version_readings,
        conditions,
        optical_letter,
        on_active_region,
        evidence,
    }))
}

/// Which kernel set sits in this volume's root directory, if any. The set
/// is required whole: one file of a pair recognizes nothing.
fn recognize_kernel(medium: &mut Medium, offset: u64) -> Result<Option<DosKernel>> {
    for kernel in DosKernel::CLAIMED {
        let mut present = true;
        for file in kernel.files() {
            match medium.stat(offset, file) {
                Ok(Some(_)) => {}
                Ok(None) => {
                    present = false;
                    break;
                }
                // A volume this seam reads no namespace on holds no
                // installation to recognize. That is an absence and not a
                // failure: a swap partition, an unrecognized filesystem
                // and an empty extent all answer the same way, and the
                // volume itself stands in the report either way.
                Err(_) => return Ok(None),
            }
        }
        if present {
            return Ok(Some(kernel));
        }
    }
    Ok(None)
}

/// What the version sources settle between them: agreement establishes,
/// disagreement stands as undetermined, and silence is its own answer.
fn settle(readings: &[VersionReading]) -> DosVersion {
    let Some(first) = readings.first() else {
        return DosVersion::Unstated;
    };
    let agreed = readings
        .iter()
        .all(|reading| reading.major == first.major && reading.minor == first.minor);
    match agreed {
        true => DosVersion::Settled {
            major: first.major,
            minor: first.minor,
        },
        false => DosVersion::Undetermined,
    }
}

/// The boot record's OEM name field — bytes 3..11 of the volume's first
/// sector, as `FORMAT` wrote it.
///
/// A reading is taken only where the field's prefix is one this kernel
/// set's own `FORMAT` writes: the field is evidence about the DOS, and a
/// prefix belonging to another product says nothing about this one.
fn read_oem_version(
    medium: &mut Medium,
    offset: u64,
    kernel: DosKernel,
) -> Result<Option<VersionReading>> {
    let mut sector = [0u8; 16];
    medium.read_space_at(offset, &mut sector)?;
    let field: String = sector[3..11]
        .iter()
        .map(|byte| *byte as char)
        .collect::<String>();
    let stated = field.trim().to_owned();

    let matched = kernel
        .oem_prefixes()
        .iter()
        .any(|prefix| stated.to_ascii_uppercase().starts_with(prefix));
    if !matched {
        return Ok(None);
    }
    Ok(parse_version(&stated)
        .map(|(major, minor)| VersionReading::new(VersionSource::OemName, major, minor, &stated)))
}

/// How much of `COMMAND.COM` the banner scan reads. The banner sits in
/// the shell's string area near the front, and the bound is what keeps
/// this a bounded working set (P27) rather than a whole-file load.
const BANNER_SCAN_BYTES: u64 = 64 * 1024;

/// The version banner inside `COMMAND.COM`, where a claimed pattern reads
/// one. Absence is an answer: a shell whose banner is assembled at
/// runtime states no version here, and nothing is inferred from its size,
/// its date, or the bytes around where a banner would have been.
fn read_command_banner(medium: &mut Medium, offset: u64) -> Result<Option<VersionReading>> {
    let Some(entry) = medium.stat(offset, "COMMAND.COM")? else {
        return Ok(None);
    };
    let length = entry.size_bytes.min(BANNER_SCAN_BYTES);
    if length == 0 {
        return Ok(None);
    }
    let mut buf = vec![0u8; length as usize];
    medium.read_file_at(offset, "COMMAND.COM", 0, &mut buf)?;

    // The claimed pattern: the word "Version" followed by digits, which
    // is how every shell in the claimed span spells its own banner.
    const MARKER: &[u8] = b"Version ";
    for start in 0..buf.len().saturating_sub(MARKER.len()) {
        if &buf[start..start + MARKER.len()] != MARKER {
            continue;
        }
        let tail: String = buf[start + MARKER.len()..]
            .iter()
            .take(8)
            .map(|byte| *byte as char)
            .collect();
        if let Some((major, minor)) = parse_version(&tail) {
            let stated = tail
                .trim_end_matches(|c: char| !c.is_ascii_digit())
                .to_owned();
            return Ok(Some(VersionReading::new(
                VersionSource::CommandBanner,
                major,
                minor,
                stated,
            )));
        }
    }
    Ok(None)
}

/// The first `<major>.<minor>` in `text`, where both sides are digits.
/// `MSDOS5.0` reads 5.0, `IBM  4.0` reads 4.0, and `6.22` reads 6.22 —
/// a one-digit minor reading as written rather than being scaled.
fn parse_version(text: &str) -> Option<(u8, u8)> {
    let bytes = text.as_bytes();
    let dot = bytes.iter().position(|byte| *byte == b'.')?;
    if dot == 0 {
        return None;
    }
    let major = (bytes[dot - 1] as char).to_digit(10)? as u8;
    let minor: String = bytes[dot + 1..]
        .iter()
        .take_while(|byte| byte.is_ascii_digit())
        .map(|byte| *byte as char)
        .collect();
    if minor.is_empty() {
        return None;
    }
    minor.parse::<u8>().ok().map(|minor| (major, minor))
}

/// The block-device drivers this release recognizes by name as adding or
/// displacing a letter. A `DEVICE=` line loading anything else — a mouse,
/// a memory manager, a display driver — moves no letter and is not
/// reported as though it might.
const BLOCK_DEVICE_DRIVERS: [&str; 6] = [
    "RAMDRIVE.SYS",
    "VDISK.SYS",
    "DBLSPACE.SYS",
    "DRVSPACE.SYS",
    "ASPIDISK.SYS",
    "SSTOR.SYS",
];

/// The network redirectors this release recognizes by name.
const NETWORK_REDIRECTORS: [&str; 5] = ["NET", "NETX", "VLM", "IPXODI", "LSL"];

/// Reads `CONFIG.SYS` and `AUTOEXEC.BAT` for the conditions that move,
/// add or ceiling a letter.
///
/// Both are absent more often than not, and absence is an answer: a
/// machine that declared nothing is a machine whose letters the claimed
/// rules assign without qualification.
fn read_conditions(
    medium: &mut Medium,
    offset: u64,
    kernel: DosKernel,
    evidence: &mut Vec<String>,
) -> Result<(Vec<ResidentCondition>, Option<char>)> {
    let mut conditions = Vec::new();
    let mut optical = None;

    for (candidates, is_config) in [
        (kernel.config_files(), true),
        (kernel.autoexec_files(), false),
    ] {
        // The first file present is the one this DOS reads; the rest are
        // not read, so they are not merged in behind it.
        let mut found = None;
        for candidate in candidates {
            if let Some(text) = read_text(medium, offset, candidate)? {
                found = Some((*candidate, text));
                break;
            }
        }
        let Some((file, text)) = found else {
            evidence.push(format!(
                "the volume holds none of {}, which {kernel} would have read",
                candidates.join(", ")
            ));
            continue;
        };
        match candidates.len() > 1 {
            true => evidence.push(format!(
                "{file} was read for what it declares; {kernel} prefers it over \
                 the others it would accept"
            )),
            false => evidence.push(format!("{file} was read for what it declares")),
        }

        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with(';') {
                continue;
            }
            let upper = line.to_ascii_uppercase();
            if !is_config {
                if let Some(letter) = mscdex_letter(&upper) {
                    evidence.push(format!(
                        "{file} runs MSCDEX with /L:{letter}, placing an optical \
                         drive at that letter"
                    ));
                    optical = Some(letter);
                }
            }
            let found = match is_config {
                true => config_condition(&upper),
                false => autoexec_condition(&upper),
            };
            if let Some(condition) = found {
                evidence.push(format!("{file} declares {}: {line}", condition.name()));
                if !conditions.contains(&condition) {
                    conditions.push(condition);
                }
            }
        }
    }

    Ok((conditions, optical))
}

/// The drive letter an `MSCDEX` line places an optical drive at — its
/// `/L:` switch. Absent the switch MSCDEX takes the first free letter,
/// which depends on what the rest of the machine took, so nothing is
/// inferred from its absence.
fn mscdex_letter(upper: &str) -> Option<char> {
    let first = upper.split_whitespace().next()?;
    let command = first.rsplit(['\\', '/']).next()?;
    if !matches!(command, "MSCDEX" | "MSCDEX.EXE") {
        return None;
    }
    let at = upper.find("/L:")?;
    let letter = upper[at + 3..].chars().next()?;
    letter.is_ascii_alphabetic().then_some(letter)
}

/// What one `CONFIG.SYS` line declares, where it declares anything a
/// claimed rule cannot model.
fn config_condition(upper: &str) -> Option<ResidentCondition> {
    if let Some(ceiling) = upper.strip_prefix("LASTDRIVE=") {
        let letter = ceiling.trim().chars().next()?;
        return letter
            .is_ascii_alphabetic()
            .then(|| ResidentCondition::LastDrive(letter.to_ascii_uppercase()));
    }
    // FreeDOS's letter-order switch is deliberately *not* read here.
    // `DLASORT`/`DLA` is not a CONFIG.SYS directive at all — FreeDOS's
    // own directive table has no entry for it — it is patched into
    // KERNEL.SYS by `SYS CONFIG` and read from there at boot. Parsing it
    // out of a configuration file would be a claim that never fires.
    let loads = upper.starts_with("DEVICE=") || upper.starts_with("DEVICEHIGH=");
    if !loads {
        return None;
    }
    // A CD-ROM driver declares its device name with /D:, which is the
    // convention MSCDEX later matches on; and a named block driver adds a
    // letter of its own. Anything else loaded here moves no letter.
    let named = BLOCK_DEVICE_DRIVERS
        .iter()
        .any(|driver| upper.contains(driver));
    (named || upper.contains("/D:")).then_some(ResidentCondition::BlockDeviceDriver)
}

/// What one `AUTOEXEC.BAT` line runs, where it runs something a claimed
/// rule cannot model. The first word is the command, so `SUBSTITUTE.EXE`
/// is not `SUBST`.
fn autoexec_condition(upper: &str) -> Option<ResidentCondition> {
    // The command is the line's first word, less any path that qualifies
    // it: `C:\DOS\SUBST.EXE D: C:\WORK` is SUBST, and `SUBSTITUTE.EXE` is
    // not.
    let first = upper.split_whitespace().next()?;
    let command = first.rsplit(['\\', '/']).next()?;
    match command {
        "SUBST" | "SUBST.EXE" => Some(ResidentCondition::Subst),
        "JOIN" | "JOIN.EXE" => Some(ResidentCondition::Join),
        "ASSIGN" | "ASSIGN.COM" => Some(ResidentCondition::Assign),
        // MSCDEX is only unmodellable when it did not say where it went.
        // With `/L:` the machine records the placement exactly, and that
        // reading is the answer rather than a reason to doubt every
        // letter.
        "MSCDEX" | "MSCDEX.EXE" => mscdex_letter(upper)
            .is_none()
            .then_some(ResidentCondition::BlockDeviceDriver),
        other => NETWORK_REDIRECTORS
            .contains(&other.trim_end_matches(".EXE").trim_end_matches(".COM"))
            .then_some(ResidentCondition::NetworkRedirector),
    }
}

/// Reads a small configuration file as text, or answers its absence.
/// These are line-oriented ASCII files of a few hundred bytes; a file
/// larger than the bound is read up to it rather than refused, the
/// declarations being at the front.
fn read_text(medium: &mut Medium, offset: u64, path: &str) -> Result<Option<String>> {
    const BOUND: u64 = 64 * 1024;
    let Some(entry) = medium.stat(offset, path)? else {
        return Ok(None);
    };
    let length = entry.size_bytes.min(BOUND);
    if length == 0 {
        return Ok(Some(String::new()));
    }
    let mut buf = vec![0u8; length as usize];
    medium.read_file_at(offset, path, 0, &mut buf)?;
    Ok(Some(buf.iter().map(|byte| *byte as char).collect()))
}

fn refusal(rule: InstallRule, reason: impl Into<String>) -> Error {
    Error::unsupported(reason).broke_rule(rule.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_rule_spells_itself_stably() {
        for rule in InstallRule::ALL {
            assert!(!rule.as_str().is_empty(), "{rule} spells itself");
        }
        assert_eq!(InstallRule::NoKernel.as_str(), "no-dos-kernel");
        assert_eq!(
            InstallRule::VersionUndetermined.as_str(),
            "dos-version-undetermined"
        );
    }

    /// The kernel set is the evidence, and each entry names the files it
    /// requires so a recognition can quote what recognized it.
    #[test]
    fn every_kernel_set_names_the_files_that_recognize_it() {
        assert_eq!(DosKernel::MsDos.files(), &["IO.SYS", "MSDOS.SYS"]);
        assert_eq!(DosKernel::PcDos.files(), &["IBMBIO.COM", "IBMDOS.COM"]);
        assert_eq!(DosKernel::FreeDos.files(), &["KERNEL.SYS"]);
        for kernel in DosKernel::CLAIMED {
            assert!(!kernel.files().is_empty(), "{kernel} names its files");
        }
    }

    fn installed(kernel: DosKernel, version: DosVersion) -> DosInstallation {
        DosInstallation {
            volume: VolumeId::of(1),
            kernel,
            version,
            version_readings: Vec::new(),
            conditions: Vec::new(),
            optical_letter: None,
            on_active_region: true,
            evidence: Vec::new(),
        }
    }

    /// FreeDOS letters as MS-DOS 5 does, and its own version number says
    /// nothing about that — 1.3 is not an MS-DOS 1.3, and asking the
    /// version would refuse a DOS this release does claim.
    #[test]
    fn freedos_letters_by_its_kernel_rather_than_its_version() {
        assert_eq!(
            installed(
                DosKernel::FreeDos,
                DosVersion::Settled { major: 1, minor: 3 }
            )
            .assignment_rule()
            .expect("FreeDOS letters as MS-DOS 5 does"),
            DosAssignmentRule::MsDos5
        );
        assert_eq!(
            installed(DosKernel::FreeDos, DosVersion::Unstated)
                .assignment_rule()
                .expect("and does so whether or not a version was read"),
            DosAssignmentRule::MsDos5
        );
    }

    /// FreeDOS reads its own configuration file ahead of the shared one,
    /// so a volume carrying both is read the way FreeDOS would read it.
    #[test]
    fn each_dos_names_the_startup_files_it_actually_reads() {
        assert_eq!(
            DosKernel::FreeDos.config_files(),
            &["FDCONFIG.SYS", "CONFIG.SYS"]
        );
        assert_eq!(
            DosKernel::FreeDos.autoexec_files(),
            &["FDAUTO.BAT", "AUTOEXEC.BAT"]
        );
        assert_eq!(DosKernel::MsDos.config_files(), &["CONFIG.SYS"]);
        assert_eq!(DosKernel::PcDos.autoexec_files(), &["AUTOEXEC.BAT"]);
    }

    /// The alternate kernel order is a declared condition, not a second
    /// rule: only the mode this release cannot model is reported, and
    /// the MS-DOS-order setting changes nothing.
    #[test]
    fn the_alternate_kernel_letter_order_is_read_only_where_it_alters_anything() {
        assert_eq!(
            config_condition("DLASORT=1"),
            None,
            "DLASORT is no CONFIG.SYS directive: FreeDOS patches it into \
             KERNEL.SYS, so reading it from here would never fire"
        );
        assert_eq!(
            config_condition("LASTDRIVE=M"),
            Some(ResidentCondition::LastDrive('M'))
        );
        assert_eq!(
            config_condition("DEVICE=C:\\DOS\\MOUSE.SYS"),
            None,
            "a driver that adds no drive moves no letter"
        );
        assert_eq!(
            config_condition("DEVICEHIGH=C:\\DOS\\RAMDRIVE.SYS 2048"),
            Some(ResidentCondition::BlockDeviceDriver)
        );
    }

    /// The command is the line's first word less any path qualifying it,
    /// so a program whose name merely starts with a claimed one is not
    /// mistaken for it.
    #[test]
    fn an_autoexec_command_is_read_as_its_own_name() {
        assert_eq!(
            autoexec_condition("C:\\DOS\\SUBST.EXE D: C:\\WORK"),
            Some(ResidentCondition::Subst)
        );
        assert_eq!(
            autoexec_condition("JOIN D: C:\\DRIVED"),
            Some(ResidentCondition::Join)
        );
        assert_eq!(autoexec_condition("SUBSTITUTE.EXE"), None);
        assert_eq!(
            autoexec_condition("NETX"),
            Some(ResidentCondition::NetworkRedirector)
        );
        assert_eq!(autoexec_condition("ECHO OFF"), None);
    }

    /// The version is settled the way geometry is: agreement establishes,
    /// disagreement stands with both readings, silence is its own fact.
    #[test]
    fn the_version_sources_settle_agree_disagree_and_silence_apart() {
        assert_eq!(settle(&[]), DosVersion::Unstated);
        assert_eq!(
            settle(&[
                VersionReading::new(VersionSource::OemName, 5, 0, "MSDOS5.0"),
                VersionReading::new(VersionSource::CommandBanner, 5, 0, "5.00"),
            ]),
            DosVersion::Settled { major: 5, minor: 0 }
        );
        assert_eq!(
            settle(&[
                VersionReading::new(VersionSource::OemName, 5, 0, "MSDOS5.0"),
                VersionReading::new(VersionSource::CommandBanner, 6, 22, "6.22"),
            ]),
            DosVersion::Undetermined,
            "neither reading is preferred by being listed first"
        );
    }

    /// P3: a DOS outside the claimed span is refused by name rather than
    /// served by the nearest claimed rule.
    #[test]
    fn a_version_outside_the_claim_is_refused_by_name() {
        assert_eq!(
            DosVersion::Settled { major: 4, minor: 1 }
                .assignment_rule()
                .expect("claimed"),
            DosAssignmentRule::MsDos4
        );
        assert_eq!(
            DosVersion::Settled {
                major: 6,
                minor: 22
            }
            .assignment_rule()
            .expect("claimed"),
            DosAssignmentRule::MsDos5
        );

        let error = DosVersion::Settled {
            major: 3,
            minor: 30,
        }
        .assignment_rule()
        .expect_err("3.30 is outside the claim");
        assert_eq!(error.rule(), Some(InstallRule::VersionNotClaimed.as_str()));
        assert!(
            error.to_string().contains("3.30"),
            "names what was found: {error}"
        );

        let error = DosVersion::Undetermined
            .assignment_rule()
            .expect_err("no rule follows from a version never settled");
        assert_eq!(
            error.rule(),
            Some(InstallRule::VersionUndetermined.as_str())
        );
    }
}
