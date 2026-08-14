// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The machine-level report: a caller states a machine and the library
//! reads everything else off the disks in it.
//!
//! These build their own images, so nothing here needs a fixture. What
//! they prove is the journey's shape — that no DOS fact is asserted by
//! the caller, and that what could not be established says so.

use std::fs::File;

use remanence::{BootOutcome, DosKernel, DosVersion, Format, HardDrive, Session};

mod dos_letters;
use dos_letters::{fat16_volume, synthetic_multi_mbr_active, write_image};

/// A COMMAND.COM whose banner states the version, which is one of the two
/// sources the version is settled from.
fn command_com(version: &str) -> Vec<u8> {
    format!("\x00\x00MS-DOS Version {version}\r\n$\x00\x00").into_bytes()
}

const CONFIG_SYS: &[u8] = b"DEVICE=C:\\DOS\\HIMEM.SYS\r\nLASTDRIVE=M\r\nFILES=30\r\n";

/// A disk whose one primary holds an MS-DOS 5 installation.
fn ms_dos_disk(tag: &str, label: &str) -> std::path::PathBuf {
    let volume = fat16_volume(
        label,
        &[
            ("IO", "SYS", b"kernel"),
            ("MSDOS", "SYS", b"kernel"),
            ("COMMAND", "COM", &command_com("5.00")),
            ("CONFIG", "SYS", CONFIG_SYS),
        ],
    );
    write_image(tag, synthetic_multi_mbr_active(&[(0x06, &volume)], 0))
}

fn seat_disk(session: &mut Session, machine: &str, path: &std::path::PathBuf) {
    let media = session
        .load_media(
            File::open(path).expect("the image is there"),
            Format::Raw {
                device: HardDrive::MbrSector.into(),
                block_bytes: 512,
            },
        )
        .expect("the image loads")
        .id();
    session
        .machine_mut(machine)
        .expect("the machine is there")
        .add_device(HardDrive::MbrSector)
        .expect("a hard disk is added")
        .insert(media)
        .expect("the disk goes in");
}

/// The whole journey: build a machine, put a disk in it, ask what it is.
/// Which DOS, which volume booted, and what its CONFIG.SYS declared are
/// all read — the caller states none of them.
#[test]
fn a_machine_reads_its_own_dos_and_letters_from_it() {
    let path = ms_dos_disk("msdos-boot", "SYSTEM");
    let mut session = Session::new();
    session.add_machine("pc").expect("a fresh identity");
    seat_disk(&mut session, "pc", &path);

    let report = session
        .machine_mut("pc")
        .expect("still here")
        .inspect()
        .expect("the machine reads");

    let BootOutcome::Booted {
        installation,
        declared,
        attachment,
    } = &report.boot
    else {
        panic!("one disk holds one system, so it booted: {:?}", report.boot);
    };
    assert_eq!(attachment, "hdd0");
    assert!(!declared, "the evidence settled it, not a declaration");
    assert_eq!(installation.kernel, DosKernel::MsDos);
    assert_eq!(
        installation.version,
        DosVersion::Settled { major: 5, minor: 0 },
        "read from COMMAND.COM's own banner"
    );

    // The LASTDRIVE the machine's own CONFIG.SYS declared, read rather
    // than asserted.
    assert!(
        installation
            .conditions
            .contains(&remanence::ResidentCondition::LastDrive('M')),
        "CONFIG.SYS declared LASTDRIVE=M: {:?}",
        installation.conditions
    );

    // And the letter, naming the volume by the identity a file verb takes.
    let c = report.volume_at('C').expect("C: names the boot volume");
    assert_eq!(
        report.letter_of(c),
        Some('C'),
        "the volume and the letter agree in both directions"
    );

    let volumes = report.volumes();
    assert_eq!(volumes.len(), 1, "one volume on one disk");
    assert_eq!(volumes[0].letter, Some('C'));
    assert_eq!(volumes[0].attachment, "hdd0");

    std::fs::remove_file(&path).ok();
}

/// A machine with nothing bootable in it establishes no letters and says
/// which devices it looked at, rather than lettering by position.
#[test]
fn a_machine_with_no_system_establishes_no_letters() {
    let volume = fat16_volume("DATA", &[("README", "TXT", b"no kernel here")]);
    let path = write_image("no-system", synthetic_multi_mbr_active(&[(0x06, &volume)], 0));

    let mut session = Session::new();
    session.add_machine("pc").expect("a fresh identity");
    seat_disk(&mut session, "pc", &path);

    let report = session
        .machine_mut("pc")
        .expect("still here")
        .inspect()
        .expect("the machine reads");

    assert_eq!(report.boot, BootOutcome::NothingBootable);
    assert!(report.drives.is_empty(), "no rule applied, so no letters");
    assert_eq!(report.volumes().len(), 1, "the volume still stands");
    assert_eq!(report.volumes()[0].letter, None);
    assert!(
        report
            .provenance
            .iter()
            .any(|line| line.contains("no volume in this machine holds an operating system")),
        "the report says why: {:?}",
        report.provenance
    );

    std::fs::remove_file(&path).ok();
}

/// Two bootable disks: the era's firmware order boots the first attached,
/// which is a claimed rule like any other — and a machine whose host set
/// its firmware otherwise declares that, overriding the evidence.
#[test]
fn the_first_attached_bootable_disk_boots_unless_the_machine_declares_otherwise() {
    let first = ms_dos_disk("dual-0", "FIRST");
    let second = ms_dos_disk("dual-1", "SECOND");

    let mut session = Session::new();
    session.add_machine("pc").expect("a fresh identity");
    seat_disk(&mut session, "pc", &first);
    seat_disk(&mut session, "pc", &second);

    let report = session
        .machine_mut("pc")
        .expect("still here")
        .inspect()
        .expect("the machine reads");

    let BootOutcome::Booted {
        attachment,
        declared,
        ..
    } = &report.boot
    else {
        panic!("the firmware order settles it: {:?}", report.boot);
    };
    assert_eq!(attachment, "hdd0", "the first attached bootable disk");
    assert!(!declared, "the firmware rule settled it, not a declaration");
    assert!(
        report
            .provenance
            .iter()
            .any(|line| line.contains("firmware order")),
        "the report names the rule it applied: {:?}",
        report.provenance
    );
    // Both disks are lettered, in attachment order, from the booting
    // system's own rule.
    assert!(report.volume_at('C').is_some());
    assert!(report.volume_at('D').is_some());

    // The declaration is the caller's one machine fact, and it settles it.
    let mut machine = session.machine_mut("pc").expect("still here");
    let hdd1 = machine.attachments()[1];
    machine.declare_boot_device(hdd1).expect("its own device");
    let report = machine.inspect().expect("reads again");

    let BootOutcome::Booted {
        attachment,
        declared,
        ..
    } = &report.boot
    else {
        panic!("the declaration settles it: {:?}", report.boot);
    };
    assert_eq!(attachment, "hdd1");
    assert!(declared, "and it is marked as declared, not read");
    assert!(
        report
            .provenance
            .iter()
            .any(|line| line.contains("configuration, not evidence")),
        "the report says the declaration is not evidence: {:?}",
        report.provenance
    );

    std::fs::remove_file(&first).ok();
    std::fs::remove_file(&second).ok();
}

/// A machine cannot be declared to boot a device it does not hold.
#[test]
fn a_boot_declaration_names_this_machines_own_device() {
    let path = ms_dos_disk("declare-refused", "SYSTEM");
    let mut session = Session::new();
    session.add_machine("pc").expect("a fresh identity");
    session.add_machine("other").expect("a fresh identity");
    seat_disk(&mut session, "other", &path);

    let elsewhere = session
        .machine_mut("other")
        .expect("there")
        .attachments()[0];
    let error = session
        .machine_mut("pc")
        .expect("there")
        .declare_boot_device(elsewhere)
        .expect_err("another machine's hdd0 is not this one's");
    assert!(
        error.to_string().contains("no device is attached"),
        "names what was wrong: {error}"
    );

    std::fs::remove_file(&path).ok();
}
