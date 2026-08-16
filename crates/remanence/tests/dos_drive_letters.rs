// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The DOS drive-letter rules, over machines the project builds outright.
//!
//! **Every fact here is read.** A caller states a machine — its drives,
//! the order they attach, the media in them — and the library reads which
//! DOS is installed, what its startup files declared, and which volume
//! booted. No test asserts a DOS variant, a slot, or a condition, because
//! the surface that took those no longer exists.
//!
//! The disks carry a real installation for that reason: the assignment
//! rule is chosen from the version in the booting volume's own shell, so
//! a disk with no DOS on it is a disk with no letters.

use remanence::{BootOutcome, LetterOutcome, RegionRole, ResidentCondition, Session};

mod dos_letters;
use dos_letters::{
    attachment_at, dos_volume, machine_of, reason_at, seat_floppy, synthetic_extended_disk,
    synthetic_fat12_floppy, synthetic_fat16, synthetic_multi_mbr, synthetic_multi_mbr_active,
    volume_at, write_image,
};

/// The whole journey: a floppy drive, a hard disk with DOS on it, and the
/// letters that DOS gave — `A:` for the floppy, `B:` its phantom, `C:`
/// the boot volume.
///
/// The floppy is a device of the machine like any other. Nothing asserts
/// it into a slot: it is a drive the caller added, holding a medium the
/// caller loaded, and the slot is the order the drives went in.
#[test]
fn one_floppy_and_one_disk_map_to_a_b_and_c() {
    let floppy = write_image("floppy", synthetic_fat12_floppy());
    let disk = write_image(
        "one-primary",
        synthetic_multi_mbr_active(&[(0x06, &dos_volume("SYSTEM", "5.00", ""))], 0),
    );

    let mut session = Session::new();
    session.add_machine("pc").expect("a fresh identity");
    seat_floppy(&mut session, "pc", &floppy);
    dos_letters::seat(&mut session, Some("pc"), &disk);

    let report = session
        .machine_mut("pc")
        .expect("still here")
        .inspect()
        .expect("the machine reads");

    assert_eq!(
        attachment_at(&report, 'A'),
        "floppy0",
        "the floppy in slot 0"
    );
    assert!(
        matches!(
            report.letter('B').expect("B: exists").outcome,
            LetterOutcome::Phantom { of: 'A' }
        ),
        "a single-floppy machine still has two floppy letters"
    );
    assert_eq!(attachment_at(&report, 'C'), "hdd0", "the boot volume");

    std::fs::remove_file(&floppy).ok();
    std::fs::remove_file(&disk).ok();
}

/// A machine with no floppy drive has neither floppy letter — they are
/// absent rather than present-and-unsettled.
#[test]
fn a_machine_with_no_floppy_drive_has_no_a_or_b() {
    let disk = write_image(
        "no-floppy",
        synthetic_multi_mbr_active(&[(0x06, &dos_volume("SYSTEM", "5.00", ""))], 0),
    );
    let (_session, report) = machine_of(&[disk.clone()]);

    assert!(report.letter('A').is_none(), "no drive, no letter");
    assert!(report.letter('B').is_none(), "and no phantom of one");
    assert_eq!(attachment_at(&report, 'C'), "hdd0");

    std::fs::remove_file(&disk).ok();
}

/// The bootable primary of every disk is lettered before the logical
/// drives of any — the rule's order, not the report's.
#[test]
fn primaries_of_every_disk_precede_the_logical_drives_of_any() {
    let system = dos_volume("SYSTEM", "5.00", "");
    let mut chain = synthetic_extended_disk(&system, 0x05, 0x06);
    chain[446] = 0x80; // the primary is the one that boots
    let first = write_image("chain-0", chain);
    let second = write_image(
        "chain-1",
        synthetic_extended_disk(&synthetic_fat16(), 0x05, 0x06),
    );
    let (_session, report) = machine_of(&[first.clone(), second.clone()]);

    assert_eq!(attachment_at(&report, 'C'), "hdd0", "first disk's primary");
    assert_eq!(attachment_at(&report, 'D'), "hdd1", "second disk's primary");
    assert_eq!(attachment_at(&report, 'E'), "hdd0", "first disk's logical");
    assert_eq!(attachment_at(&report, 'F'), "hdd1", "second disk's logical");
    assert_eq!(report.drives.len(), 4);

    // The letters are not the order the report lists the regions in.
    assert_ne!(volume_at(&report, 'D'), volume_at(&report, 'E'));

    std::fs::remove_file(&first).ok();
    std::fs::remove_file(&second).ok();
}

/// Every claimed variant letters a disk's **bootable** primary first, not
/// simply its first row. The disk that tells the rules apart is one whose
/// active flag sits on the second primary.
#[test]
fn the_bootable_primary_takes_c_ahead_of_the_first_row() {
    let path = write_image(
        "active-second",
        synthetic_multi_mbr_active(
            &[
                (0x06, &synthetic_fat16()),
                (0x06, &dos_volume("SYSTEM", "5.00", "")),
            ],
            1,
        ),
    );
    let (_session, report) = machine_of(&[path.clone()]);

    let disk = report.disks[0].report.as_ref().expect("a medium is in it");
    let active: Vec<_> = disk
        .regions
        .iter()
        .filter(|region| region.declared_active)
        .map(|region| region.declared_number)
        .collect();
    assert_eq!(active, vec![2], "the table flags the second primary");

    let bootable = disk
        .volumes
        .iter()
        .find(|volume| match &volume.origin {
            remanence::VolumeOrigin::Regions(regions) => regions
                .iter()
                .any(|id| disk.region(*id).is_some_and(|r| r.declared_active)),
            remanence::VolumeOrigin::WholeDevice => false,
        })
        .expect("the active region composed a volume")
        .id;

    assert_eq!(
        volume_at(&report, 'C'),
        bootable,
        "C: is the bootable primary, not the first row"
    );
    assert_ne!(
        volume_at(&report, 'D'),
        bootable,
        "the first row falls to the remaining-primaries pass"
    );

    std::fs::remove_file(&path).ok();
}

/// A partition type no claimed variant letters — a hidden FAT16B — takes
/// no letter, and the letters behind it do not shift onto it.
#[test]
fn a_type_outside_the_dos_set_takes_no_letter() {
    let path = write_image(
        "hidden",
        synthetic_multi_mbr_active(
            &[
                (0x16, &synthetic_fat16()),
                (0x06, &dos_volume("SYSTEM", "5.00", "")),
            ],
            1,
        ),
    );
    let (_session, report) = machine_of(&[path.clone()]);

    assert_eq!(report.drives.len(), 1, "one lettered partition, not two");
    assert_eq!(attachment_at(&report, 'C'), "hdd0");
    assert!(report.letter('D').is_none(), "the hidden type takes none");

    // The hidden region is reported and composes no volume at all, so
    // the one volume that exists is the one that took the letter.
    let disk = report.disks[0].report.as_ref().expect("a medium is in it");
    assert!(
        disk.regions
            .iter()
            .any(|region| region.declared_type == 0x16),
        "the hidden region is still reported"
    );
    assert_eq!(report.volumes().len(), 1);
    assert_eq!(report.volumes()[0].letter, Some('C'));

    std::fs::remove_file(&path).ok();
}

/// An extended partition of a type no claimed variant follows letters
/// none of its logical drives, even though the library reads them.
#[test]
fn an_unclaimed_extended_partition_letters_none_of_its_logicals() {
    let system = dos_volume("SYSTEM", "5.00", "");
    let mut disk = synthetic_extended_disk(&system, 0x0f, 0x06);
    disk[446] = 0x80; // the primary is the one that boots
    let path = write_image("lba-ext", disk);
    let (_session, report) = machine_of(&[path.clone()]);

    assert_eq!(attachment_at(&report, 'C'), "hdd0", "the primary letters");
    assert!(report.letter('D').is_none(), "its logical does not");

    let disk = report.disks[0].report.as_ref().expect("a medium is in it");
    assert!(
        disk.regions
            .iter()
            .any(|region| region.role == RegionRole::Data
                && region.declared_placement == "logical"),
        "the library still read the chain"
    );

    std::fs::remove_file(&path).ok();
}

/// The `LASTDRIVE` ceiling is read from the machine's own `CONFIG.SYS`,
/// and the letters the rule assigns above it come back undetermined.
#[test]
fn a_lastdrive_ceiling_read_from_config_sys_unsettles_the_letters_above_it() {
    let system = dos_volume("SYSTEM", "5.00", "LASTDRIVE=C\r\nFILES=30\r\n");
    let first = write_image(
        "ceiling-0",
        synthetic_multi_mbr_active(&[(0x06, &system)], 0),
    );
    let second = write_image(
        "ceiling-1",
        synthetic_multi_mbr(&[(0x06, &synthetic_fat16())]),
    );
    let (_session, report) = machine_of(&[first.clone(), second.clone()]);

    // C: sits at the ceiling and stands; D: sits above it and does not.
    assert_eq!(attachment_at(&report, 'C'), "hdd0");
    let reason = reason_at(&report, 'D');
    assert!(
        reason.contains("LASTDRIVE=C"),
        "the reason names the ceiling that unsettled it: {reason}"
    );

    // And it was read, not asserted.
    let BootOutcome::Booted { installation, .. } = &report.boot else {
        panic!("the disk holds a system");
    };
    assert!(
        installation
            .conditions
            .contains(&ResidentCondition::LastDrive('C'))
    );

    std::fs::remove_file(&first).ok();
    std::fs::remove_file(&second).ok();
}

/// `SUBST` in the machine's own `AUTOEXEC.BAT` unsettles every letter:
/// no claimed rule models it, and which letter it moved is not knowable
/// from the disks.
#[test]
fn a_subst_read_from_autoexec_unsettles_every_letter() {
    let banner = dos_letters::command_com("6.22");
    let system = dos_letters::fat16_volume(
        "SYSTEM",
        &[
            ("IO", "SYS", b"kernel"),
            ("MSDOS", "SYS", b"kernel"),
            ("COMMAND", "COM", &banner),
            (
                "AUTOEXEC",
                "BAT",
                b"@ECHO OFF\r\nC:\\DOS\\SUBST.EXE E: C:\\WORK\r\n",
            ),
        ],
    );
    let path = write_image("subst", synthetic_multi_mbr_active(&[(0x06, &system)], 0));
    let (_session, report) = machine_of(&[path.clone()]);

    let reason = reason_at(&report, 'C');
    assert!(
        reason.contains("SUBST"),
        "every letter is unsettled, naming SUBST: {reason}"
    );

    std::fs::remove_file(&path).ok();
}

/// An optical drive takes the letter the machine's own `MSCDEX` line
/// placed it at — read from `AUTOEXEC.BAT`, not declared by a caller —
/// and the letter names the drive the machine holds.
///
/// The two halves are different facts: the device says there is a drive,
/// and the startup line says which letter it took.
#[test]
fn an_optical_drive_takes_the_letter_its_mscdex_line_placed_it_at() {
    let path = write_image(
        "mscdex",
        synthetic_multi_mbr_active(&[(0x06, &mscdex_system("/D:MSCD001 /L:R"))], 0),
    );

    let mut session = Session::new();
    session.add_machine("pc").expect("a fresh identity");
    dos_letters::seat(&mut session, Some("pc"), &path);
    let drive = dos_letters::add_optical_drive(&mut session, "pc");
    let report = session
        .machine_mut("pc")
        .expect("still here")
        .inspect()
        .expect("the machine reads");

    let (attachment, placed_by) = dos_letters::optical_at(&report, 'R');
    assert_eq!(
        attachment.as_deref(),
        Some(drive.as_str()),
        "R: names the drive the machine holds, not a letter standing alone"
    );
    assert!(
        placed_by.contains("MSCDEX"),
        "and says what placed it: {placed_by}"
    );
    assert!(
        report.volume_at('R').is_none(),
        "the library composes no volume for it, so it names none"
    );

    std::fs::remove_file(&path).ok();
}

/// A drive the machine holds that no `MSCDEX` line places takes **no**
/// letter, and is accounted for in provenance rather than left unsaid.
///
/// `MSCDEX` without `/L:` takes the first free letter, which depends on
/// what the rest of the machine took — so the silence establishes
/// nothing, and nothing is inferred from it.
#[test]
fn an_optical_drive_no_line_places_takes_no_letter_and_is_accounted_for() {
    let path = write_image(
        "no-mscdex",
        synthetic_multi_mbr_active(&[(0x06, &dos_volume("SYSTEM", "6.22", ""))], 0),
    );

    let mut session = Session::new();
    session.add_machine("pc").expect("a fresh identity");
    dos_letters::seat(&mut session, Some("pc"), &path);
    let drive = dos_letters::add_optical_drive(&mut session, "pc");
    let report = session
        .machine_mut("pc")
        .expect("still here")
        .inspect()
        .expect("the machine reads");

    assert!(
        report
            .drives
            .iter()
            .all(|mapping| !matches!(mapping.outcome, LetterOutcome::OpticalDrive { .. })),
        "no letter is guessed for a drive nothing placed"
    );
    assert!(
        report
            .provenance
            .iter()
            .any(|line| line.contains(&drive) && line.contains("takes no letter")),
        "and the drive the machine holds is accounted for: {:?}",
        report.provenance
    );
    assert!(
        report
            .disks
            .iter()
            .any(|disk| disk.attachment == drive && disk.family.as_deref() == Some("optical")),
        "the drive appears in the device set as the optical device it is"
    );

    std::fs::remove_file(&path).ok();
}

/// A machine whose startup files place an optical drive it does not hold
/// keeps both readings: the letter stands, and it names no device.
#[test]
fn a_placement_with_no_drive_behind_it_names_no_device() {
    let path = write_image(
        "mscdex-no-drive",
        synthetic_multi_mbr_active(&[(0x06, &mscdex_system("/D:MSCD001 /L:R"))], 0),
    );
    let (_session, report) = machine_of(&[path.clone()]);

    let (attachment, _) = dos_letters::optical_at(&report, 'R');
    assert_eq!(
        attachment, None,
        "the line was written by the machine that ran, and the device set is \
         the caller's; neither is preferred"
    );

    std::fs::remove_file(&path).ok();
}

/// A FAT16 system volume whose `AUTOEXEC.BAT` runs `MSCDEX` with the
/// switches given.
fn mscdex_system(switches: &str) -> Vec<u8> {
    let banner = dos_letters::command_com("6.22");
    let autoexec = format!("@ECHO OFF\r\nC:\\DOS\\MSCDEX.EXE {switches}\r\n").into_bytes();
    dos_letters::fat16_volume(
        "SYSTEM",
        &[
            ("IO", "SYS", b"kernel"),
            ("MSDOS", "SYS", b"kernel"),
            ("COMMAND", "COM", &banner),
            ("AUTOEXEC", "BAT", &autoexec),
        ],
    )
}

/// The installed version chooses the rule, and the two claimed variants
/// disagree in exactly one place: what becomes of a second primary. A
/// machine running 4.01 letters it not at all; one running 5.00 letters
/// it last.
#[test]
fn the_installed_version_chooses_the_rule_that_letters_a_second_primary() {
    for (version, expected) in [("4.01", 1usize), ("5.00", 2usize)] {
        let system = dos_volume("SYSTEM", version, "");
        let path = write_image(
            &format!("variant-{version}"),
            synthetic_multi_mbr_active(&[(0x06, &system), (0x06, &synthetic_fat16())], 0),
        );
        let (_session, report) = machine_of(&[path.clone()]);

        assert_eq!(
            report.drives.len(),
            expected,
            "MS-DOS {version} letters {expected} of the two primaries"
        );
        assert_eq!(attachment_at(&report, 'C'), "hdd0");

        std::fs::remove_file(&path).ok();
    }
}

/// A DOS outside the claimed span is refused by name rather than served
/// by the nearest claimed rule, and the machine's volumes still stand.
#[test]
fn a_version_outside_the_claim_letters_nothing_and_says_why() {
    let system = dos_volume("SYSTEM", "3.30", "");
    let path = write_image("dos330", synthetic_multi_mbr_active(&[(0x06, &system)], 0));
    let (_session, report) = machine_of(&[path.clone()]);

    assert!(report.drives.is_empty(), "no claimed rule covers DOS 3.30");
    assert!(
        report
            .provenance
            .iter()
            .any(|line| line.contains("3.30") || line.contains("no letters were assigned")),
        "the report says why: {:?}",
        report.provenance
    );
    assert_eq!(report.volumes().len(), 1, "the volume still stands");

    std::fs::remove_file(&path).ok();
}

/// The report names the rule it applied and what it applied it to, and
/// says plainly that the result is not evidence.
#[test]
fn the_report_carries_the_rule_it_applied_and_calls_it_provenance() {
    let path = write_image(
        "provenance",
        synthetic_multi_mbr_active(&[(0x06, &dos_volume("SYSTEM", "5.00", ""))], 0),
    );
    let (_session, report) = machine_of(&[path.clone()]);

    let joined = report.provenance.join("\n");
    assert!(
        joined.contains("provenance, not evidence read off a disk"),
        "a derived mapping is not evidence: {joined}"
    );
    assert!(
        joined.contains("rule ms-dos-5"),
        "it names the rule it applied: {joined}"
    );
    assert!(
        joined.contains("firmware order"),
        "and how it settled which disk booted: {joined}"
    );

    std::fs::remove_file(&path).ok();
}

/// The identity the report issues is what a file verb takes: the letter
/// is for showing a user, and the volume identity is what reaches the
/// bytes.
#[test]
fn the_identity_the_report_issued_names_the_volume_the_file_verb_reaches() {
    let system = dos_letters::fat16_volume(
        "SYSTEM",
        &[
            ("IO", "SYS", b"kernel"),
            ("MSDOS", "SYS", b"kernel"),
            ("COMMAND", "COM", &dos_letters::command_com("5.00")),
            ("HELLO", "TXT", b"reached through C:"),
        ],
    );
    let path = write_image("reach", synthetic_multi_mbr_active(&[(0x06, &system)], 0));

    let mut session = Session::new();
    session.add_machine("pc").expect("a fresh identity");
    dos_letters::seat(&mut session, Some("pc"), &path);
    let report = session
        .machine_mut("pc")
        .expect("still here")
        .inspect()
        .expect("the machine reads");

    let volume = volume_at(&report, 'C');
    let attachment = session.machine("pc").expect("there").attachments()[0];
    let mut view = session
        .machine_mut("pc")
        .expect("there")
        .into_device(attachment)
        .expect("there");
    let medium = view.medium_mut().expect("occupied");

    // The ordinal the volume identity carries is the partition the file
    // verbs are reached through.
    let mut space = medium
        .partition(1)
        .expect("the table declared it")
        .filesystem()
        .expect("FAT");
    assert_eq!(space.volume_id(), Some(volume), "the same volume");
    assert_eq!(
        space.read_file("HELLO.TXT").expect("reads"),
        b"reached through C:"
    );

    std::fs::remove_file(&path).ok();
}

/// A device family no claimed rule letters is passed over by name, not
/// silently: the report says which drive and why.
#[test]
fn a_family_no_rule_letters_is_passed_over_and_said_so() {
    let path = write_image(
        "passed-over",
        synthetic_multi_mbr_active(&[(0x06, &dos_volume("SYSTEM", "5.00", ""))], 0),
    );

    let mut session = Session::new();
    session.add_machine("pc").expect("a fresh identity");
    dos_letters::seat(&mut session, Some("pc"), &path);
    session
        .machine_mut("pc")
        .expect("there")
        .add_device(remanence::FloppyDrive::Commodore1541)
        .expect("a 1541 is a device like any other");

    let report = session
        .machine_mut("pc")
        .expect("still here")
        .inspect()
        .expect("the machine reads");

    assert_eq!(attachment_at(&report, 'C'), "hdd0", "the DOS disk letters");
    let empty = report
        .disks
        .iter()
        .find(|disk| disk.attachment.starts_with("cbmfloppy"))
        .expect("the 1541 is in the report");
    assert!(
        empty
            .note
            .as_deref()
            .is_some_and(|note| note.contains("holds no medium")),
        "an empty drive is configuration in its own right: {:?}",
        empty.note
    );

    std::fs::remove_file(&path).ok();
}

/// A machine with nothing bootable letters nothing, and every volume it
/// holds still stands in the report.
#[test]
fn a_machine_with_no_system_letters_nothing() {
    let path = write_image(
        "no-system",
        synthetic_multi_mbr(&[(0x06, &synthetic_fat16())]),
    );
    let (_session, report) = machine_of(&[path.clone()]);

    assert_eq!(report.boot, BootOutcome::NothingBootable);
    assert!(report.drives.is_empty());
    assert_eq!(report.volumes().len(), 1);
    assert_eq!(report.volumes()[0].letter, None);

    std::fs::remove_file(&path).ok();
}

/// The anonymous machine reads exactly as a named one does — it is the
/// machine whose identity is null, not one distinguished by behaviour.
#[test]
fn the_anonymous_machine_reads_like_any_other() {
    let path = write_image(
        "anonymous",
        synthetic_multi_mbr_active(&[(0x06, &dos_volume("SYSTEM", "5.00", ""))], 0),
    );

    let mut session = Session::new();
    dos_letters::seat(&mut session, None, &path);
    let report = session
        .anonymous_mut()
        .inspect()
        .expect("the anonymous machine reads");

    assert_eq!(report.machine, None, "its identity is null");
    assert_eq!(attachment_at(&report, 'C'), "hdd0");
    assert!(
        report
            .provenance
            .iter()
            .any(|line| line.contains("anonymous machine")),
        "and the report says which machine it read: {:?}",
        report.provenance
    );

    std::fs::remove_file(&path).ok();
}
