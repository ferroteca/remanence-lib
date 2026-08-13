// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The DOS drive-letter composer over synthetic machines the project owns
//! outright, and over the FreeDOS rig artifact — a real disk carrying two
//! primary partitions and an extended chain of two logicals, which is the
//! layout the claimed variants disagree about.
//!
//! The composer opens nothing: every test here inspects its images first
//! and then asserts machine facts over the reports it already holds.

use std::path::PathBuf;

use remanence::{
    DosAssignmentRule, DosMachine, FloppyDrive, Format, HardDrive, LetterOutcome,
    MachineDevice, RegionRole, ResidentCondition, Session,
};

mod dos_letters;
use dos_letters::{
    device_at, inspect, reason_at, synthetic_extended_disk, synthetic_fat12_floppy,
    synthetic_fat16, synthetic_multi_mbr, synthetic_rig_disk, volume_at, write_image,
};
use dos_letters::{attach, seat};













/// The journey U22 asks for: floppy in slot 0, one hard disk, and the
/// letters DOS would have shown — each naming a volume by the identity its
/// own report issued.
#[test]
fn one_floppy_and_one_disk_map_to_a_b_and_c() {
    let floppy_path = write_image("floppy", synthetic_fat12_floppy());
    let disk_path = write_image(
        "one-primary",
        synthetic_multi_mbr(&[(0x06, &synthetic_fat16())]),
    );
    let (_floppy_session, floppy) = inspect(
        &floppy_path,
        Format::Raw {
            device: HardDrive::MbrSector,
            block_bytes: 512,
        },
    );
    let (_disk_session, disk) = inspect(
        &disk_path,
        Format::Raw {
            device: HardDrive::MbrSector,
            block_bytes: 512,
        },
    );

    let mut machine = DosMachine::new();
    machine.assert_floppy(0, &floppy).expect("slot 0 is free");
    machine
        .assert_fixed_disk(0, &disk)
        .expect("order 0 is free");
    let map = machine
        .compose(Some(DosAssignmentRule::MsDos5))
        .expect("composes");

    assert_eq!(
        map.mappings.iter().map(|m| m.letter).collect::<Vec<_>>(),
        vec!['A', 'B', 'C'],
        "a single-floppy machine still has two floppy letters"
    );
    assert_eq!(device_at(&map, 'A'), MachineDevice::Floppy(0));
    assert_eq!(device_at(&map, 'C'), MachineDevice::FixedDisk(0));
    assert_eq!(
        map.letter('B').expect("B: is mapped").outcome,
        LetterOutcome::Phantom { of: 'A' },
        "the second letter of a single-floppy machine is the phantom drive"
    );

    // The identity is what goes back into a file verb, and the letter is
    // what a consumer shows a user.
    assert!(
        floppy.volume(volume_at(&map, 'A')).is_some(),
        "A: names a volume of the floppy's own report"
    );
    assert!(
        disk.volume(volume_at(&map, 'C')).is_some(),
        "C: names a volume of the disk's own report"
    );

    std::fs::remove_file(&floppy_path).ok();
    std::fs::remove_file(&disk_path).ok();
}

/// A machine with no floppy drive has no A: or B: at all, which is a
/// different answer from a letter that exists and could not be settled.
#[test]
fn a_diskless_of_floppies_machine_has_no_a_or_b() {
    let disk_path = write_image(
        "floppyless",
        synthetic_multi_mbr(&[(0x06, &synthetic_fat16())]),
    );
    let (_session, disk) = inspect(
        &disk_path,
        Format::Raw {
            device: HardDrive::MbrSector,
            block_bytes: 512,
        },
    );

    let mut machine = DosMachine::new();
    machine
        .assert_fixed_disk(0, &disk)
        .expect("order 0 is free");
    let map = machine
        .compose(Some(DosAssignmentRule::MsDos5))
        .expect("composes");

    assert!(map.letter('A').is_none(), "no floppy drive, no A:");
    assert!(map.letter('B').is_none(), "and no phantom of one");
    assert_eq!(device_at(&map, 'C'), MachineDevice::FixedDisk(0));

    std::fs::remove_file(&disk_path).ok();
}

/// The order is the rule's, not the report's: every disk's first primary
/// comes before any disk's logical drives.
#[test]
fn primaries_of_every_disk_precede_the_logical_drives_of_any() {
    let fat = synthetic_fat16();
    let first = write_image("chain-0", synthetic_extended_disk(&fat, 0x05, 0x06));
    let second = write_image("chain-1", synthetic_extended_disk(&fat, 0x05, 0x06));
    let (_first_session, first_report) = inspect(
        &first,
        Format::Raw {
            device: HardDrive::MbrSector,
            block_bytes: 512,
        },
    );
    let (_second_session, second_report) = inspect(
        &second,
        Format::Raw {
            device: HardDrive::MbrSector,
            block_bytes: 512,
        },
    );

    let mut machine = DosMachine::new();
    machine.assert_fixed_disk(0, &first_report).expect("free");
    machine.assert_fixed_disk(1, &second_report).expect("free");
    let map = machine
        .compose(Some(DosAssignmentRule::MsDos4))
        .expect("composes");

    assert_eq!(
        device_at(&map, 'C'),
        MachineDevice::FixedDisk(0),
        "first primary"
    );
    assert_eq!(
        device_at(&map, 'D'),
        MachineDevice::FixedDisk(1),
        "second disk's primary"
    );
    assert_eq!(
        device_at(&map, 'E'),
        MachineDevice::FixedDisk(0),
        "first disk's logical"
    );
    assert_eq!(
        device_at(&map, 'F'),
        MachineDevice::FixedDisk(1),
        "second disk's logical"
    );
    assert_eq!(map.mappings.len(), 4);

    // The letters are not the order the report lists the regions in: the
    // first disk's logical is D: in the report's order and E: under DOS.
    assert_ne!(volume_at(&map, 'D'), volume_at(&map, 'E'));

    std::fs::remove_file(&first).ok();
    std::fs::remove_file(&second).ok();
}

/// A partition type no claimed variant letters — a hidden FAT16B — takes
/// no letter, and the letters behind it do not shift onto it.
#[test]
fn a_type_outside_the_dos_set_takes_no_letter() {
    let fat = synthetic_fat16();
    let path = write_image("hidden", synthetic_multi_mbr(&[(0x16, &fat), (0x06, &fat)]));
    let (_session, report) = inspect(
        &path,
        Format::Raw {
            device: HardDrive::MbrSector,
            block_bytes: 512,
        },
    );

    let mut machine = DosMachine::new();
    machine.assert_fixed_disk(0, &report).expect("free");
    let map = machine
        .compose(Some(DosAssignmentRule::MsDos5))
        .expect("composes");

    assert_eq!(map.mappings.len(), 1, "only the FAT16B primary is lettered");
    let region = report
        .regions
        .iter()
        .find(|region| region.declared_type == 0x06)
        .expect("the disk declares a FAT16B primary");
    let volume = report
        .volumes
        .iter()
        .find(|volume| volume.origin == remanence::VolumeOrigin::Regions(vec![region.id]))
        .expect("it composed");
    assert_eq!(
        volume_at(&map, 'C'),
        volume.id,
        "C: is the DOS-typed primary"
    );

    std::fs::remove_file(&path).ok();
}

/// An LBA-addressed extended partition is one this library reads and the
/// claimed variants do not follow, so its logical drives take no letter —
/// and the map says so rather than leaving the caller to notice.
#[test]
fn an_unclaimed_extended_partition_letters_none_of_its_logicals() {
    let fat = synthetic_fat16();
    let path = write_image("lba-chain", synthetic_extended_disk(&fat, 0x0f, 0x06));
    let (_session, report) = inspect(
        &path,
        Format::Raw {
            device: HardDrive::MbrSector,
            block_bytes: 512,
        },
    );

    let mut machine = DosMachine::new();
    machine.assert_fixed_disk(0, &report).expect("free");
    let map = machine
        .compose(Some(DosAssignmentRule::MsDos5))
        .expect("composes");

    assert_eq!(map.mappings.len(), 1, "the primary alone takes a letter");
    assert!(
        map.provenance
            .iter()
            .any(|line| line.contains("extended partition is not type 0x05")),
        "the map says why the chain took none: {:?}",
        map.provenance
    );

    std::fs::remove_file(&path).ok();
}

/// A declared LASTDRIVE ceiling is outside every claimed rule, so the
/// letters above it are undetermined rather than assigned anyway.
#[test]
fn a_declared_lastdrive_ceiling_unsettles_the_letters_above_it() {
    let fat = synthetic_fat16();
    let path = write_image(
        "ceiling",
        synthetic_multi_mbr(&[(0x06, &fat), (0x06, &fat)]),
    );
    let (_session, report) = inspect(
        &path,
        Format::Raw {
            device: HardDrive::MbrSector,
            block_bytes: 512,
        },
    );

    let mut machine = DosMachine::new();
    machine.assert_fixed_disk(0, &report).expect("free");
    machine.declare(ResidentCondition::LastDrive('C'));
    let map = machine
        .compose(Some(DosAssignmentRule::MsDos5))
        .expect("composes");

    assert!(
        matches!(
            map.letter('C').expect("mapped").outcome,
            LetterOutcome::Volume { .. }
        ),
        "C: sits at the ceiling and stands"
    );
    assert!(
        reason_at(&map, 'D').contains("LASTDRIVE"),
        "D: sits above it and says why it is unsettled"
    );

    std::fs::remove_file(&path).ok();
}

/// SUBST could have redirected any letter, and no claimed rule models it:
/// every letter comes back undetermined rather than approximated.
#[test]
fn a_declared_subst_unsettles_every_letter() {
    let fat = synthetic_fat16();
    let path = write_image("subst", synthetic_multi_mbr(&[(0x06, &fat)]));
    let (_session, report) = inspect(
        &path,
        Format::Raw {
            device: HardDrive::MbrSector,
            block_bytes: 512,
        },
    );

    let mut machine = DosMachine::new();
    machine.assert_fixed_disk(0, &report).expect("free");
    machine.declare(ResidentCondition::Subst);
    let map = machine
        .compose(Some(DosAssignmentRule::MsDos5))
        .expect("composes");

    assert_eq!(
        map.established_count(),
        0,
        "nothing is established under SUBST"
    );
    assert!(reason_at(&map, 'C').contains("SUBST"));

    std::fs::remove_file(&path).ok();
}

/// A CD-ROM takes a letter only where the caller declares where the
/// resident driver put it, and a declaration contradicting the rule is a
/// refusal rather than a silent winner.
#[test]
fn a_cd_rom_letter_follows_only_a_declared_placement() {
    let fat = synthetic_fat16();
    let path = write_image("cdrom", synthetic_multi_mbr(&[(0x06, &fat)]));
    let (_session, report) = inspect(
        &path,
        Format::Raw {
            device: HardDrive::MbrSector,
            block_bytes: 512,
        },
    );

    let mut undeclared = DosMachine::new();
    undeclared.assert_fixed_disk(0, &report).expect("free");
    undeclared.assert_cdrom(0, None).expect("free");
    let map = undeclared
        .compose(Some(DosAssignmentRule::MsDos5))
        .expect("composes");
    assert_eq!(
        map.mappings.len(),
        1,
        "an undeclared CD-ROM takes no letter"
    );
    assert!(
        map.provenance
            .iter()
            .any(|line| line.contains("cd-rom drive 0 takes no letter")),
        "and the map says why: {:?}",
        map.provenance
    );

    let mut declared = DosMachine::new();
    declared.assert_fixed_disk(0, &report).expect("free");
    declared.assert_cdrom(0, Some('e')).expect("free");
    let map = declared
        .compose(Some(DosAssignmentRule::MsDos5))
        .expect("composes");
    assert_eq!(
        device_at(&map, 'E'),
        MachineDevice::CdRom(0),
        "a declared letter is normalized and honored"
    );
    assert!(
        matches!(
            map.letter('E').expect("mapped").outcome,
            LetterOutcome::DeclaredDevice { .. }
        ),
        "the library composes no volume for a CD-ROM and invents no identity"
    );

    let mut contradictory = DosMachine::new();
    contradictory.assert_fixed_disk(0, &report).expect("free");
    contradictory.assert_cdrom(0, Some('C')).expect("free");
    let error = contradictory
        .compose(Some(DosAssignmentRule::MsDos5))
        .expect_err("contradictory facts are refused");
    assert!(
        error.to_string().contains("contradict"),
        "the refusal names the contradiction: {error}"
    );

    std::fs::remove_file(&path).ok();
}

/// Machine facts are stated once, and a slot DOS does not have is refused
/// by name rather than lettered past B:.
#[test]
fn contradictory_machine_facts_are_refused_by_name() {
    let fat = synthetic_fat16();
    let floppy_path = write_image("dup-floppy", synthetic_fat12_floppy());
    let path = write_image("dup-disk", synthetic_multi_mbr(&[(0x06, &fat)]));
    let (_floppy_session, floppy) = inspect(
        &floppy_path,
        Format::Raw {
            device: HardDrive::MbrSector,
            block_bytes: 512,
        },
    );
    let (_session, report) = inspect(
        &path,
        Format::Raw {
            device: HardDrive::MbrSector,
            block_bytes: 512,
        },
    );

    let mut machine = DosMachine::new();
    machine.assert_floppy(0, &floppy).expect("free");
    let error = machine.assert_floppy(0, &floppy).expect_err("stated twice");
    assert!(error.to_string().contains("floppy slot 0"), "{error}");

    let error = machine
        .assert_floppy(2, &floppy)
        .expect_err("no third slot");
    assert!(error.to_string().contains("outside the claim"), "{error}");

    machine.assert_fixed_disk(0, &report).expect("free");
    let error = machine
        .assert_fixed_disk(0, &report)
        .expect_err("stated twice");
    assert!(error.to_string().contains("fixed disk 0"), "{error}");

    // A machine whose only floppy sits in the second slot is not a
    // machine, and no rule is applied to one.
    let mut second_only = DosMachine::new();
    second_only.assert_floppy(1, &floppy).expect("free");
    let error = second_only
        .compose(Some(DosAssignmentRule::MsDos5))
        .expect_err("no first floppy");
    assert!(error.to_string().contains("without slot 0"), "{error}");

    std::fs::remove_file(&floppy_path).ok();
    std::fs::remove_file(&path).ok();
}

// ---------------------------------------------------------------------------
// The FreeDOS rig artifact: two primaries and an extended chain of two
// logicals on one real disk — the layout the claimed variants differ over.

/// The layout the claimed variants differ over, built rather than
/// downloaded: two DOS primaries and an extended chain of two
/// logicals. `rig_layout.rs` is what says the built disk is that
/// shape; these tests are what the shape is for.
fn rig_artifact(tag: &str) -> PathBuf {
    write_image(tag, synthetic_rig_disk())
}

/// Stating the variant settles the whole map: under MS-DOS 5 the disk's
/// second primary takes a letter, after every logical drive.
#[test]
fn a_stated_variant_letters_the_second_primary_last() {
    let path = rig_artifact("stated");
    let (_session, report) = inspect(
        &path,
        Format::Raw {
            device: HardDrive::MbrBlock,
            block_bytes: 512,
        },
    );

    let mut machine = DosMachine::new();
    machine.assert_fixed_disk(0, &report).expect("free");
    let map = machine
        .compose(Some(DosAssignmentRule::MsDos5))
        .expect("composes");

    assert_eq!(map.established_count(), 4, "two primaries and two logicals");
    let primaries: Vec<_> = report
        .regions
        .iter()
        .filter(|region| region.declared_placement == "primary" && region.role == RegionRole::Data)
        .collect();
    assert_eq!(primaries.len(), 2, "the rig disk carries two DOS primaries");
    assert_eq!(
        volume_at(&map, 'C'),
        report
            .volumes
            .iter()
            .find(|volume| volume.origin == remanence::VolumeOrigin::Regions(vec![primaries[0].id]))
            .expect("composed")
            .id,
        "C: is the first primary"
    );
    assert_eq!(
        volume_at(&map, 'F'),
        report
            .volumes
            .iter()
            .find(|volume| volume.origin == remanence::VolumeOrigin::Regions(vec![primaries[1].id]))
            .expect("composed")
            .id,
        "the second primary comes after both logical drives"
    );

    // Under MS-DOS 4 the same disk has three letters and the second
    // primary has none at all.
    let map = machine
        .compose(Some(DosAssignmentRule::MsDos4))
        .expect("composes");
    assert_eq!(map.mappings.len(), 3, "MS-DOS 4 letters no second primary");
    assert!(map.letter('F').is_none());

    std::fs::remove_file(&path).ok();
}

/// Where the caller states no variant, the letters the claimed rules
/// agree on stand and the one they disagree on is undetermined — never
/// settled by choosing the more common rule.
#[test]
fn an_unstated_variant_leaves_the_disputed_letter_undetermined() {
    let path = rig_artifact("unstated");
    let (_session, report) = inspect(
        &path,
        Format::Raw {
            device: HardDrive::MbrBlock,
            block_bytes: 512,
        },
    );

    let mut machine = DosMachine::new();
    machine.assert_fixed_disk(0, &report).expect("free");
    let map = machine.compose(None).expect("composes");

    assert_eq!(map.applied_rules, DosAssignmentRule::CLAIMED.to_vec());
    assert_eq!(map.established_count(), 3, "the agreed letters stand");
    for letter in ['C', 'D', 'E'] {
        assert!(
            matches!(
                map.letter(letter).expect("mapped").outcome,
                LetterOutcome::Volume { .. }
            ),
            "{letter}: is agreed by every claimed rule"
        );
    }
    let reason = reason_at(&map, 'F');
    assert!(
        reason.contains("ms-dos-5"),
        "the reason names each rule: {reason}"
    );
    assert!(
        reason.contains("ms-dos-4"),
        "the reason names each rule: {reason}"
    );
    assert!(
        reason.contains("assigns no letter"),
        "and says what the disagreement is: {reason}"
    );

    std::fs::remove_file(&path).ok();
}

/// The letter is what a consumer shows a user; the identity is what it
/// passes back into a file verb. This is the whole point of the mapping,
/// so it is exercised end to end on a real image.
///
/// **The identity the mapping issued still names exactly the volume the
/// file verb reaches.** The file verbs live on the partition door now
/// (P19), so the round trip runs letter → the identity the map issued →
/// the one partition of the pool whose composed volume carries that
/// identity → its namespace → the file. That the two ends still meet is
/// U4's identity claim surviving the move: an identity names the same
/// volume wherever it is met, and the door it is met through does not
/// change which volume that is (P21).
#[test]
fn the_identity_the_mapping_issued_names_the_volume_the_file_verb_reaches() {
    let path = rig_artifact("file-verb");
    let (mut session, attachment) = attach(
        &path,
        Format::Raw {
            device: HardDrive::MbrBlock,
            block_bytes: 512,
        },
    );
    let report = session
        .medium_mut(attachment)
        .expect("the medium is pooled")
        .inspect()
        .expect("inspection reads");

    let mut machine = DosMachine::new();
    machine.assert_fixed_disk(0, &report).expect("free");
    let map = machine
        .compose(Some(DosAssignmentRule::MsDos5))
        .expect("composes");
    let lettered = volume_at(&map, 'C');

    let medium = session
        .medium_mut(attachment)
        .expect("the medium is pooled");
    let named: Vec<u32> = medium
        .partitions()
        .iter()
        .filter(|partition| partition.volume_id() == Some(lettered))
        .map(|partition| partition.ordinal())
        .collect();
    assert_eq!(
        named.len(),
        1,
        "the identity C: carries names exactly one partition's volume: {named:?}"
    );

    let marker = medium
        .partition(named[0])
        .expect("the pool bears the partition that identity named")
        .filesystem()
        .expect("a DOS data partition determines FAT")
        .read_file("RMNMARK.TXT")
        .expect("C: reads through the identity the map returned");
    assert!(marker.starts_with(b"remanence marker:"));

    drop(session);
    std::fs::remove_file(&path).ok();
}

/// The mapping carries its provenance and never calls it evidence: the
/// asserted facts and the applied rules travel with the answer.
#[test]
fn the_map_carries_the_asserted_facts_and_the_rule_it_applied() {
    let path = rig_artifact("provenance");
    let (_session, report) = inspect(
        &path,
        Format::Raw {
            device: HardDrive::MbrSector,
            block_bytes: 512,
        },
    );

    let mut machine = DosMachine::new();
    machine.assert_fixed_disk(0, &report).expect("free");
    let map = machine
        .compose(Some(DosAssignmentRule::MsDos5))
        .expect("composes");

    assert!(
        map.provenance
            .iter()
            .any(|line| line.contains("provenance, not evidence")),
        "a derived mapping states that it is not evidence: {:?}",
        map.provenance
    );
    assert!(
        map.provenance.iter().any(|line| line.contains("ms-dos-5")),
        "and names the rule it applied"
    );
    assert!(
        map.provenance
            .iter()
            .any(|line| line.contains("fixed disk 0")),
        "and the machine facts it was given"
    );

    std::fs::remove_file(&path).ok();
}

/// P32's other half: with a device tier holding the machine facts, the
/// composer reads them from a machine's own device set instead of from an
/// assertion — attachment order being the order its devices were added.
#[test]
fn a_machine_letters_its_own_device_set_in_attachment_order() {
    let fat = synthetic_fat16();
    let first = write_image("set-0", synthetic_multi_mbr(&[(0x06, &fat)]));
    let second = write_image("set-1", synthetic_multi_mbr(&[(0x06, &fat)]));

    let mut session = Session::new();
    session.add_machine("pc").expect("the machine is added");
    for path in [&first, &second] {
        seat(&mut session, Some("pc"), path);
    }

    let map = session
        .machine_mut("pc")
        .expect("is there")
        .compose_dos_letters(Some(DosAssignmentRule::MsDos5), &[])
        .expect("composes");

    assert_eq!(device_at(&map, 'C'), MachineDevice::FixedDisk(0));
    assert_eq!(device_at(&map, 'D'), MachineDevice::FixedDisk(1));
    assert!(map.letter('A').is_none(), "no floppy family is claimed");

    // The provenance says where the facts came from, which is the whole
    // difference between this form and the asserted one (P35).
    assert!(
        map.provenance
            .iter()
            .any(|line| line.contains("device set") && line.contains("'pc'")),
        "names the machine it read: {:?}",
        map.provenance
    );
    assert!(
        map.provenance
            .iter()
            .any(|line| line.contains("hdd0, hdd1")),
        "and the devices, in attachment order: {:?}",
        map.provenance
    );

    drop(session);
    std::fs::remove_file(&first).ok();
    std::fs::remove_file(&second).ok();
}

/// A family no claimed rule letters is passed over by family — never an
/// error, never a silent omission — and an empty drive contributes no
/// volume. P32 names the case: an attached `cbmfloppy0` legitimately
/// receives no DOS letter.
#[test]
fn a_family_no_rule_letters_is_passed_over_and_said_so() {
    let path = write_image(
        "passed-over",
        synthetic_multi_mbr(&[(0x06, &synthetic_fat16())]),
    );

    let mut session = Session::new();
    session
        .add_device(FloppyDrive::Commodore1541)
        .expect("a 1541 is a device like any other");
    seat(&mut session, None, &path);
    session
        .add_device(HardDrive::MbrSector)
        .expect("an empty second drive is configuration in its own right");

    let map = session
        .anonymous_mut()
        .compose_dos_letters(Some(DosAssignmentRule::MsDos5), &[])
        .expect("composes");

    assert_eq!(device_at(&map, 'C'), MachineDevice::FixedDisk(0));
    assert!(map.letter('D').is_none(), "the empty drive holds no volume");
    assert!(
        map.provenance
            .iter()
            .any(|line| line.contains("passed over by family") && line.contains("cbmfloppy0")),
        "the 1541 is passed over, and the map says so: {:?}",
        map.provenance
    );
    assert!(
        map.provenance
            .iter()
            .any(|line| line.contains("no medium") && line.contains("hdd1")),
        "and the empty drive is accounted for rather than dropped: {:?}",
        map.provenance
    );
    assert!(
        map.provenance
            .iter()
            .any(|line| line.contains("anonymous machine")),
        "the anonymous machine composes like any other (D23)"
    );

    drop(session);
    std::fs::remove_file(&path).ok();
}
