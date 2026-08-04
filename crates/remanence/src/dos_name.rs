// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The DOS 8.3 namespace: every name decision the file-access seam makes,
//! in the one place that can check them against the format.
//!
//! Three decisions live here and nowhere else. **Reading** a stored name
//! ([`read`]) undoes the format's fixed-width padding and its escape for a
//! leading `0xE5` byte, and returns what the directory actually holds.
//! **Matching** ([`matches`]) compares a caller's name to a stored one the
//! way DOS did, without regard to case. **Storing** ([`store`]) validates
//! and normalizes: the caller supplies the name it has, and this uppercases,
//! pads, and hands back the eleven bytes a directory record takes.
//!
//! A name outside the namespace is refused, never truncated, transliterated,
//! or repaired to fit (P6), and the refusal names which of the seven rules it
//! broke ([`DosNameRule`], P10). The rules are checked in a fixed order —
//! structure, then content, then the reserved names — so one name always
//! breaks the same rule.

use std::fmt;

use crate::error::{Error, Result, RuleIdentity};

/// One rule of the DOS 8.3 namespace, as an enumerated claim (P3).
///
/// This is the set a refusal's rule identity is drawn from, owned here
/// rather than by the error type: the category on the error says how a
/// caller should behave, and this says which rule the name broke, so a
/// caller can state it in its own words without parsing a diagnostic or
/// reimplementing the namespace to decide what the diagnostic would have
/// said.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DosNameRule {
    /// Nothing before the extension: `""`, `"."`, `".TXT"`.
    EmptyBase,
    /// More than eight characters before the separator.
    BaseTooLong,
    /// More than three characters after it.
    ExtensionTooLong,
    /// More than one separator, or one where the format allows none —
    /// a name ending in the separator has nothing to separate.
    Separator,
    /// A character the namespace excludes, named by the refusal.
    ExcludedCharacter,
    /// A device name DOS resolves ahead of any file on a volume, with or
    /// without an extension: `CON`, `PRN`, `AUX`, `NUL`, `COM1`–`COM9`,
    /// `LPT1`–`LPT9`.
    ReservedDeviceName,
    /// A leading or trailing space in the base or the extension, which the
    /// format's own space padding would silently swallow.
    SurroundingSpace,
}

impl DosNameRule {
    /// Every rule in the set, in the order they are checked.
    pub const ALL: [Self; 7] = [
        Self::Separator,
        Self::EmptyBase,
        Self::BaseTooLong,
        Self::ExtensionTooLong,
        Self::SurroundingSpace,
        Self::ExcludedCharacter,
        Self::ReservedDeviceName,
    ];

    /// The stable cross-language spelling of this rule, which is what a
    /// refusal carries.
    pub const fn as_str(self) -> RuleIdentity {
        match self {
            Self::EmptyBase => "empty-base",
            Self::BaseTooLong => "base-too-long",
            Self::ExtensionTooLong => "extension-too-long",
            Self::Separator => "separator",
            Self::ExcludedCharacter => "excluded-character",
            Self::ReservedDeviceName => "reserved-device-name",
            Self::SurroundingSpace => "surrounding-space",
        }
    }

    /// Reads a rule identity back into this set, for a caller branching on
    /// [`Error::rule`](crate::Error::rule). An identity from another seam's
    /// set — or from a later revision of this one — is `None` rather than a
    /// nearest match.
    pub fn from_identity(identity: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|rule| rule.as_str() == identity)
    }
}

impl fmt::Display for DosNameRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The characters the namespace admits beside `A`–`Z` and `0`–`9`.
const PUNCTUATION: [char; 16] = [
    '!', '#', '$', '%', '&', '\'', '(', ')', '-', '@', '^', '_', '`', '{', '}', '~',
];

/// The device names DOS resolves ahead of any file on a volume. The claim
/// is this set alone (P3): `COM0` and `LPT0` are not among them.
const RESERVED_DEVICES: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// The stored escape for a name whose first byte is `0xE5`, which the
/// format spends on marking a record deleted. It encodes a stored name and
/// is never a rule a caller can break, so it stays inside this module.
const DELETED_ESCAPE: u8 = 0x05;
const DELETED: u8 = 0xe5;

fn refusal(rule: DosNameRule, reason: String) -> Error {
    Error::io(reason).broke_rule(rule.as_str())
}

fn admits(character: char) -> bool {
    character.is_ascii_uppercase() || character.is_ascii_digit() || PUNCTUATION.contains(&character)
}

/// Validates and normalizes one caller-supplied name into the eleven bytes
/// a directory record holds: the base padded to eight, the extension to
/// three, uppercased. Nothing is repaired to fit — a name outside the
/// namespace is refused, naming the rule.
pub(crate) fn store(name: &str) -> Result<[u8; 11]> {
    let upper = name.to_ascii_uppercase();

    // Structure first: how the name divides decides what the rest reads.
    let separators = upper.matches('.').count();
    if separators > 1 {
        return Err(refusal(
            DosNameRule::Separator,
            format!(
                "'{name}' has {separators} separators; a DOS 8.3 name has at \
                 most one, between the base and the extension"
            ),
        ));
    }
    let (base, extension) = upper.split_once('.').unwrap_or((upper.as_str(), ""));
    if base.is_empty() {
        return Err(refusal(
            DosNameRule::EmptyBase,
            format!(
                "'{name}' has an empty base name; a DOS 8.3 name needs at \
                 least one character before the separator"
            ),
        ));
    }
    if separators == 1 && extension.is_empty() {
        return Err(refusal(
            DosNameRule::Separator,
            format!(
                "'{name}' ends in the separator, which has nothing to \
                 separate; a DOS 8.3 name with no extension carries none"
            ),
        ));
    }
    let base_length = base.chars().count();
    if base_length > 8 {
        return Err(refusal(
            DosNameRule::BaseTooLong,
            format!(
                "'{name}' has a {base_length}-character base name; a DOS 8.3 \
                 name allows at most 8"
            ),
        ));
    }
    let extension_length = extension.chars().count();
    if extension_length > 3 {
        return Err(refusal(
            DosNameRule::ExtensionTooLong,
            format!(
                "'{name}' has a {extension_length}-character extension; a DOS \
                 8.3 name allows at most 3"
            ),
        ));
    }

    // Then content, the surrounding space before the general character
    // rule: it is the one the format's own space padding would swallow
    // without a trace, so it is worth naming on its own.
    for (part, which) in [(base, "base name"), (extension, "extension")] {
        if part.starts_with(' ') || part.ends_with(' ') {
            return Err(refusal(
                DosNameRule::SurroundingSpace,
                format!(
                    "'{name}' has a leading or trailing space in its {which}; \
                     a DOS 8.3 name allows none, the format's padding being \
                     spaces itself"
                ),
            ));
        }
    }
    if let Some(character) = base.chars().chain(extension.chars()).find(|c| !admits(*c)) {
        return Err(refusal(
            DosNameRule::ExcludedCharacter,
            format!(
                "'{name}' contains '{}', which the DOS 8.3 namespace excludes",
                character.escape_debug()
            ),
        ));
    }

    // Last, the names that are well-formed and still cannot be a file.
    if let Some(device) = RESERVED_DEVICES.iter().find(|device| **device == base) {
        return Err(refusal(
            DosNameRule::ReservedDeviceName,
            format!(
                "'{name}' names the reserved device '{device}', which DOS \
                 resolves ahead of any file on a volume, extension or not"
            ),
        ));
    }

    // Every character admitted is ASCII, so a character count is a byte
    // count and the two fixed-width fields take what was measured.
    let mut raw = [b' '; 11];
    raw[..base.len()].copy_from_slice(base.as_bytes());
    raw[8..8 + extension.len()].copy_from_slice(extension.as_bytes());
    Ok(raw)
}

/// Reads the eleven bytes of a directory record's name field back as the
/// name the directory holds: padding removed, the `0xE5` escape undone, and
/// the separator restored only where there is an extension to separate.
pub(crate) fn read(field: &[u8]) -> String {
    debug_assert!(field.len() >= 11, "a name field is eleven bytes");
    let mut base: Vec<u8> = field[..8].to_vec();
    if base[0] == DELETED_ESCAPE {
        base[0] = DELETED;
    }
    let base = String::from_utf8_lossy(&base).trim_end().to_string();
    let extension = String::from_utf8_lossy(&field[8..11]).trim_end().to_string();
    if extension.is_empty() {
        base
    } else {
        format!("{base}.{extension}")
    }
}

/// Whether a caller's name reaches a stored one. DOS matched without regard
/// to case, so this does; the stored name is what a listing returns, and
/// nothing here rewrites either side.
pub(crate) fn matches(stored: &str, requested: &str) -> bool {
    stored.eq_ignore_ascii_case(requested)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stored(name: &str) -> String {
        String::from_utf8(store(name).expect("a storable name").to_vec()).expect("ASCII")
    }

    fn broken_rule(name: &str) -> DosNameRule {
        let error = store(name).expect_err("a refusal");
        DosNameRule::from_identity(error.rule().expect("a rule identity"))
            .expect("a rule of this set")
    }

    #[test]
    fn a_caller_supplies_the_name_it_has_and_the_seam_stores_the_dos_one() {
        assert_eq!(stored("x.txt"), "X       TXT");
        assert_eq!(stored("Out"), "OUT        ");
        assert_eq!(stored("readme.1"), "README  1  ");
        assert_eq!(stored("ABCDEFGH.IJK"), "ABCDEFGHIJK");
    }

    #[test]
    fn every_rule_of_the_set_is_reachable_and_names_itself() {
        assert_eq!(broken_rule(""), DosNameRule::EmptyBase);
        assert_eq!(broken_rule("."), DosNameRule::EmptyBase);
        assert_eq!(broken_rule(".txt"), DosNameRule::EmptyBase);
        assert_eq!(broken_rule("LONGFILENAME.TXT"), DosNameRule::BaseTooLong);
        assert_eq!(broken_rule("index.html"), DosNameRule::ExtensionTooLong);
        assert_eq!(broken_rule("a.b.c"), DosNameRule::Separator);
        assert_eq!(broken_rule("file."), DosNameRule::Separator);
        assert_eq!(broken_rule("my file.txt"), DosNameRule::ExcludedCharacter);
        assert_eq!(broken_rule("a+b.txt"), DosNameRule::ExcludedCharacter);
        assert_eq!(broken_rule("café.txt"), DosNameRule::ExcludedCharacter);
        assert_eq!(broken_rule(" file.txt"), DosNameRule::SurroundingSpace);
        assert_eq!(broken_rule("file .txt"), DosNameRule::SurroundingSpace);
        assert_eq!(broken_rule("file. tx"), DosNameRule::SurroundingSpace);
        // Structure is checked before content, so a space that also
        // overruns the field is the length rule rather than this one.
        assert_eq!(broken_rule("file. txt"), DosNameRule::ExtensionTooLong);
        assert_eq!(broken_rule("con"), DosNameRule::ReservedDeviceName);
        assert_eq!(broken_rule("CON.txt"), DosNameRule::ReservedDeviceName);
        assert_eq!(broken_rule("lpt9"), DosNameRule::ReservedDeviceName);
    }

    #[test]
    fn a_reserved_name_is_the_enumerated_set_and_not_a_prefix_of_it() {
        assert_eq!(stored("com0"), "COM0       ");
        assert_eq!(stored("lpt0"), "LPT0       ");
        assert_eq!(stored("console"), "CONSOLE    ");
        assert_eq!(stored("con1"), "CON1       ");
    }

    #[test]
    fn a_refusal_says_what_was_found_beside_the_rule_it_names() {
        let error = store("index.html").expect_err("a refusal");
        assert_eq!(error.rule(), Some(DosNameRule::ExtensionTooLong.as_str()));
        assert_eq!(
            error.to_string(),
            "'index.html' has a 4-character extension; a DOS 8.3 name allows at most 3"
        );
        let error = store("a+b.txt").expect_err("a refusal");
        assert_eq!(
            error.to_string(),
            "'a+b.txt' contains '+', which the DOS 8.3 namespace excludes"
        );
    }

    #[test]
    fn a_stored_name_reads_back_as_the_directory_holds_it() {
        assert_eq!(read(b"X       TXT     "), "X.TXT");
        assert_eq!(read(b"OUT            "), "OUT");
        assert_eq!(read(b"\x05IGMA   DAT"), "\u{fffd}IGMA.DAT");
    }

    #[test]
    fn matching_ignores_case_and_nothing_else() {
        assert!(matches("README.TXT", "readme.txt"));
        assert!(matches("README.TXT", "README.TXT"));
        assert!(!matches("README.TXT", "README"));
        assert!(!matches("README.TXT", "READ.TXT"));
    }

    #[test]
    fn rule_spellings_are_stable() {
        assert_eq!(DosNameRule::EmptyBase.as_str(), "empty-base");
        assert_eq!(DosNameRule::BaseTooLong.as_str(), "base-too-long");
        assert_eq!(DosNameRule::ExtensionTooLong.as_str(), "extension-too-long");
        assert_eq!(DosNameRule::Separator.as_str(), "separator");
        assert_eq!(DosNameRule::ExcludedCharacter.as_str(), "excluded-character");
        assert_eq!(
            DosNameRule::ReservedDeviceName.as_str(),
            "reserved-device-name"
        );
        assert_eq!(DosNameRule::SurroundingSpace.as_str(), "surrounding-space");
        for rule in DosNameRule::ALL {
            assert_eq!(DosNameRule::from_identity(rule.as_str()), Some(rule));
        }
        assert_eq!(DosNameRule::from_identity("not-a-rule"), None);
    }
}
