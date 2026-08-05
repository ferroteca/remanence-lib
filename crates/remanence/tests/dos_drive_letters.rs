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
    AccessIntent, AttachmentId, DeviceFamily, DiskReport, DosAssignmentRule, DosMachine,
    LetterOutcome,
    MachineDevice, RegionRole, ResidentCondition, Session, VolumeId,
};

mod common;

fn attach(path: impl AsRef<std::path::Path>) -> (Session, AttachmentId) {
    let mut session = Session::new();
    let device = session
        .add_device(DeviceFamily::HARD_DISK)
        .expect("the drive is added");
    let attachment = device.attachment();
    device
        .load_media(path, AccessIntent::Read)
        .expect("the image loads");
    (session, attachment)
}

fn temp_path(tag: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nonce = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "remanence-letters-{tag}-{}-{nonce}.img",
        std::process::id()
    ))
}

/// A minimal FAT16 volume: 512-byte sectors, 1 sector/cluster, 2 FATs of
/// 32 sectors, 512 root entries, 8000 total sectors.
fn synthetic_fat16() -> Vec<u8> {
    const TOTAL_SECTORS: usize = 8000;
    let mut image = vec![0u8; TOTAL_SECTORS * 512];

    image[0] = 0xeb;
    image[1] = 0x3c;
    image[2] = 0x90;
    image[3..11].copy_from_slice(b"REMANENC");
    image[11..13].copy_from_slice(&512u16.to_le_bytes());
    image[13] = 1;
    image[14..16].copy_from_slice(&1u16.to_le_bytes());
    image[16] = 2;
    image[17..19].copy_from_slice(&512u16.to_le_bytes());
    image[19..21].copy_from_slice(&(TOTAL_SECTORS as u16).to_le_bytes());
    image[21] = 0xf8;
    image[22..24].copy_from_slice(&32u16.to_le_bytes());
    image[24..26].copy_from_slice(&18u16.to_le_bytes());
    image[26..28].copy_from_slice(&2u16.to_le_bytes());
    image[510] = 0x55;
    image[511] = 0xaa;

    for fat in 0..2usize {
        let base = (1 + fat * 32) * 512;
        image[base..base + 2].copy_from_slice(&0xfff8u16.to_le_bytes());
        image[base + 2..base + 4].copy_from_slice(&0xffffu16.to_le_bytes());
    }

    image
}

/// A 1.44M-floppy-shaped FAT12 volume, bare on the medium as a floppy is.
fn synthetic_fat12_floppy() -> Vec<u8> {
    const TOTAL_SECTORS: usize = 2880;
    let mut image = vec![0u8; TOTAL_SECTORS * 512];

    image[0] = 0xeb;
    image[1] = 0x3c;
    image[2] = 0x90;
    image[3..11].copy_from_slice(b"REMANENC");
    image[11..13].copy_from_slice(&512u16.to_le_bytes());
    image[13] = 1;
    image[14..16].copy_from_slice(&1u16.to_le_bytes());
    image[16] = 2;
    image[17..19].copy_from_slice(&224u16.to_le_bytes());
    image[19..21].copy_from_slice(&(TOTAL_SECTORS as u16).to_le_bytes());
    image[21] = 0xf0;
    image[22..24].copy_from_slice(&9u16.to_le_bytes());
    image[24..26].copy_from_slice(&18u16.to_le_bytes());
    image[26..28].copy_from_slice(&2u16.to_le_bytes());
    image[510] = 0x55;
    image[511] = 0xaa;

    for fat in 0..2usize {
        let base = (1 + fat * 9) * 512;
        image[base] = 0xf0;
        image[base + 1] = 0xff;
        image[base + 2] = 0xff;
    }

    image
}

/// An MBR disk with one primary slot per entry, placed consecutively.
fn synthetic_multi_mbr(entries: &[(u8, &[u8])]) -> Vec<u8> {
    let mut start_lba = 2048usize;
    let mut layout = Vec::new();
    for (type_byte, volume) in entries {
        let sectors = volume.len() / 512;
        layout.push((*type_byte, start_lba, sectors, *volume));
        start_lba += sectors;
    }

    let mut disk = vec![0u8; start_lba * 512];
    for (slot, (type_byte, start, sectors, volume)) in layout.iter().enumerate() {
        let at = 446 + slot * 16;
        disk[at + 4] = *type_byte;
        disk[at + 8..at + 12].copy_from_slice(&(*start as u32).to_le_bytes());
        disk[at + 12..at + 16].copy_from_slice(&(*sectors as u32).to_le_bytes());
        disk[start * 512..start * 512 + volume.len()].copy_from_slice(volume);
    }
    disk[510] = 0x55;
    disk[511] = 0xaa;
    disk
}

/// A disk with one primary of `primary_type` and an extended chain of one
/// logical volume, the container declared as `container_type`.
fn synthetic_extended_disk(volume: &[u8], container_type: u8, primary_type: u8) -> Vec<u8> {
    let sectors = volume.len() / 512;
    let primary_start = 2048usize;
    let ext_base = primary_start + sectors;
    let link_span = 2048 + sectors;
    let mut disk = vec![0u8; (ext_base + link_span) * 512];

    disk[446 + 4] = primary_type;
    disk[446 + 8..446 + 12].copy_from_slice(&(primary_start as u32).to_le_bytes());
    disk[446 + 12..446 + 16].copy_from_slice(&(sectors as u32).to_le_bytes());
    disk[462 + 4] = container_type;
    disk[462 + 8..462 + 12].copy_from_slice(&(ext_base as u32).to_le_bytes());
    disk[462 + 12..462 + 16].copy_from_slice(&(link_span as u32).to_le_bytes());
    disk[510] = 0x55;
    disk[511] = 0xaa;
    disk[primary_start * 512..primary_start * 512 + volume.len()].copy_from_slice(volume);

    let ebr = ext_base * 512;
    disk[ebr + 446 + 4] = 0x06;
    disk[ebr + 446 + 8..ebr + 446 + 12].copy_from_slice(&2048u32.to_le_bytes());
    disk[ebr + 446 + 12..ebr + 446 + 16].copy_from_slice(&(sectors as u32).to_le_bytes());
    disk[ebr + 510] = 0x55;
    disk[ebr + 511] = 0xaa;
    let logical = (ext_base + 2048) * 512;
    disk[logical..logical + volume.len()].copy_from_slice(volume);

    disk
}

fn write_image(tag: &str, bytes: Vec<u8>) -> PathBuf {
    let path = temp_path(tag);
    std::fs::write(&path, bytes).expect("image writes");
    path
}

/// Inspects an image and hands back the report, keeping the session alive
/// for as long as the caller holds it.
fn inspect(path: &PathBuf) -> (Session, DiskReport) {
    let (mut session, attachment) = attach(path);
    let report = session
        .require_device(attachment)
        .expect("the medium is attached")
        .inspect()
        .expect("inspection reads");
    (session, report)
}

fn volume_at(map: &remanence::DriveMap, letter: char) -> VolumeId {
    match &map.letter(letter).unwrap_or_else(|| panic!("{letter}: is mapped")).outcome {
        LetterOutcome::Volume { volume, .. } => *volume,
        other => panic!("{letter}: names a volume, not {}", other.name()),
    }
}

fn device_at(map: &remanence::DriveMap, letter: char) -> MachineDevice {
    match &map.letter(letter).unwrap_or_else(|| panic!("{letter}: is mapped")).outcome {
        LetterOutcome::Volume { device, .. } | LetterOutcome::DeclaredDevice { device } => *device,
        other => panic!("{letter}: names a device, not {}", other.name()),
    }
}

fn reason_at(map: &remanence::DriveMap, letter: char) -> String {
    match &map.letter(letter).unwrap_or_else(|| panic!("{letter}: is mapped")).outcome {
        LetterOutcome::Undetermined { reason } => reason.clone(),
        other => panic!("{letter}: is undetermined, not {}", other.name()),
    }
}

/// The journey U22 asks for: floppy in slot 0, one hard disk, and the
/// letters DOS would have shown — each naming a volume by the identity its
/// own report issued.
#[test]
fn one_floppy_and_one_disk_map_to_a_b_and_c() {
    let floppy_path = write_image("floppy", synthetic_fat12_floppy());
    let disk_path = write_image("one-primary", synthetic_multi_mbr(&[(0x06, &synthetic_fat16())]));
    let (_floppy_session, floppy) = inspect(&floppy_path);
    let (_disk_session, disk) = inspect(&disk_path);

    let mut machine = DosMachine::new();
    machine.assert_floppy(0, &floppy).expect("slot 0 is free");
    machine.assert_fixed_disk(0, &disk).expect("order 0 is free");
    let map = machine.compose(Some(DosAssignmentRule::MsDos5)).expect("composes");

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
    let disk_path = write_image("floppyless", synthetic_multi_mbr(&[(0x06, &synthetic_fat16())]));
    let (_session, disk) = inspect(&disk_path);

    let mut machine = DosMachine::new();
    machine.assert_fixed_disk(0, &disk).expect("order 0 is free");
    let map = machine.compose(Some(DosAssignmentRule::MsDos5)).expect("composes");

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
    let (_first_session, first_report) = inspect(&first);
    let (_second_session, second_report) = inspect(&second);

    let mut machine = DosMachine::new();
    machine.assert_fixed_disk(0, &first_report).expect("free");
    machine.assert_fixed_disk(1, &second_report).expect("free");
    let map = machine.compose(Some(DosAssignmentRule::MsDos4)).expect("composes");

    assert_eq!(device_at(&map, 'C'), MachineDevice::FixedDisk(0), "first primary");
    assert_eq!(device_at(&map, 'D'), MachineDevice::FixedDisk(1), "second disk's primary");
    assert_eq!(device_at(&map, 'E'), MachineDevice::FixedDisk(0), "first disk's logical");
    assert_eq!(device_at(&map, 'F'), MachineDevice::FixedDisk(1), "second disk's logical");
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
    let (_session, report) = inspect(&path);

    let mut machine = DosMachine::new();
    machine.assert_fixed_disk(0, &report).expect("free");
    let map = machine.compose(Some(DosAssignmentRule::MsDos5)).expect("composes");

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
    assert_eq!(volume_at(&map, 'C'), volume.id, "C: is the DOS-typed primary");

    std::fs::remove_file(&path).ok();
}

/// An LBA-addressed extended container is one this library reads and the
/// claimed variants do not follow, so its logical drives take no letter —
/// and the map says so rather than leaving the caller to notice.
#[test]
fn an_unclaimed_extended_container_letters_none_of_its_logicals() {
    let fat = synthetic_fat16();
    let path = write_image("lba-chain", synthetic_extended_disk(&fat, 0x0f, 0x06));
    let (_session, report) = inspect(&path);

    let mut machine = DosMachine::new();
    machine.assert_fixed_disk(0, &report).expect("free");
    let map = machine.compose(Some(DosAssignmentRule::MsDos5)).expect("composes");

    assert_eq!(map.mappings.len(), 1, "the primary alone takes a letter");
    assert!(
        map.provenance
            .iter()
            .any(|line| line.contains("extended container is not type 0x05")),
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
    let path = write_image("ceiling", synthetic_multi_mbr(&[(0x06, &fat), (0x06, &fat)]));
    let (_session, report) = inspect(&path);

    let mut machine = DosMachine::new();
    machine.assert_fixed_disk(0, &report).expect("free");
    machine.declare(ResidentCondition::LastDrive('C'));
    let map = machine.compose(Some(DosAssignmentRule::MsDos5)).expect("composes");

    assert!(
        matches!(map.letter('C').expect("mapped").outcome, LetterOutcome::Volume { .. }),
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
    let (_session, report) = inspect(&path);

    let mut machine = DosMachine::new();
    machine.assert_fixed_disk(0, &report).expect("free");
    machine.declare(ResidentCondition::Subst);
    let map = machine.compose(Some(DosAssignmentRule::MsDos5)).expect("composes");

    assert_eq!(map.established_count(), 0, "nothing is established under SUBST");
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
    let (_session, report) = inspect(&path);

    let mut undeclared = DosMachine::new();
    undeclared.assert_fixed_disk(0, &report).expect("free");
    undeclared.assert_cdrom(0, None).expect("free");
    let map = undeclared.compose(Some(DosAssignmentRule::MsDos5)).expect("composes");
    assert_eq!(map.mappings.len(), 1, "an undeclared CD-ROM takes no letter");
    assert!(
        map.provenance.iter().any(|line| line.contains("cd-rom drive 0 takes no letter")),
        "and the map says why: {:?}",
        map.provenance
    );

    let mut declared = DosMachine::new();
    declared.assert_fixed_disk(0, &report).expect("free");
    declared.assert_cdrom(0, Some('e')).expect("free");
    let map = declared.compose(Some(DosAssignmentRule::MsDos5)).expect("composes");
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
    let (_floppy_session, floppy) = inspect(&floppy_path);
    let (_session, report) = inspect(&path);

    let mut machine = DosMachine::new();
    machine.assert_floppy(0, &floppy).expect("free");
    let error = machine.assert_floppy(0, &floppy).expect_err("stated twice");
    assert!(error.to_string().contains("floppy slot 0"), "{error}");

    let error = machine.assert_floppy(2, &floppy).expect_err("no third slot");
    assert!(error.to_string().contains("outside the claim"), "{error}");

    machine.assert_fixed_disk(0, &report).expect("free");
    let error = machine.assert_fixed_disk(0, &report).expect_err("stated twice");
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

fn rig_artifact(tag: &str) -> PathBuf {
    let master = common::ensure_fixture("freedos-parttest.qcow2");
    let copy = std::env::temp_dir().join(format!(
        "remanence-letters-{tag}-{}.qcow2",
        std::process::id()
    ));
    std::fs::copy(master, &copy).expect("artifact copies");
    copy
}

/// Stating the variant settles the whole map: under MS-DOS 5 the disk's
/// second primary takes a letter, after every logical drive.
#[test]
fn a_stated_variant_letters_the_second_primary_last() {
    let path = rig_artifact("stated");
    let (_session, report) = inspect(&path);

    let mut machine = DosMachine::new();
    machine.assert_fixed_disk(0, &report).expect("free");
    let map = machine.compose(Some(DosAssignmentRule::MsDos5)).expect("composes");

    assert_eq!(map.established_count(), 4, "two primaries and two logicals");
    let primaries: Vec<_> = report
        .regions
        .iter()
        .filter(|region| {
            region.declared_placement == "primary" && region.role == RegionRole::Data
        })
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
    let map = machine.compose(Some(DosAssignmentRule::MsDos4)).expect("composes");
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
    let (_session, report) = inspect(&path);

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
    assert!(reason.contains("ms-dos-5"), "the reason names each rule: {reason}");
    assert!(reason.contains("ms-dos-4"), "the reason names each rule: {reason}");
    assert!(
        reason.contains("assigns no letter"),
        "and says what the disagreement is: {reason}"
    );

    std::fs::remove_file(&path).ok();
}

/// The letter is what a consumer shows a user; the identity is what it
/// passes back into a file verb. This is the whole point of the mapping,
/// so it is exercised end to end on a real image.
#[test]
fn a_letters_identity_addresses_the_volume_in_a_file_verb() {
    let path = rig_artifact("file-verb");
    let (mut session, attachment) = attach(&path);
    let report = session
        .require_device(attachment)
        .expect("attached")
        .inspect()
        .expect("inspection reads");

    let mut machine = DosMachine::new();
    machine.assert_fixed_disk(0, &report).expect("free");
    let map = machine.compose(Some(DosAssignmentRule::MsDos5)).expect("composes");

    let marker = session
        .require_device(attachment)
        .expect("attached")
        .read_file(volume_at(&map, 'C'), "RMNMARK.TXT")
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
    let (_session, report) = inspect(&path);

    let mut machine = DosMachine::new();
    machine.assert_fixed_disk(0, &report).expect("free");
    let map = machine.compose(Some(DosAssignmentRule::MsDos5)).expect("composes");

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
    let machine = session.require_machine("pc").expect("is there");
    for path in [&first, &second] {
        machine
            .add_device(DeviceFamily::HARD_DISK)
            .expect("a hard disk is added")
            .load_media(path, AccessIntent::Read)
            .expect("the disk loads");
    }

    let map = machine
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
    let path = write_image("passed-over", synthetic_multi_mbr(&[(0x06, &synthetic_fat16())]));

    let mut session = Session::new();
    session
        .add_device(DeviceFamily::COMMODORE_1541)
        .expect("a 1541 is a device like any other");
    session
        .add_device(DeviceFamily::HARD_DISK)
        .expect("added")
        .load_media(&path, AccessIntent::Read)
        .expect("the disk loads");
    session
        .add_device(DeviceFamily::HARD_DISK)
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
