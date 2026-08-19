// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! PC-DOS 1.00, read from the release that wrote no parameter block.
//!
//! **This is the suite that makes the media-descriptor table
//! answerable.** A 1.x boot sector states nothing a reader can look up,
//! so every value the layout applies is a claim about the world. What
//! stops it from being unfalsifiable is a real IBM distribution disk and
//! files whose contents say for themselves whether the layout was right.
//!
//! A wrong `sectors_per_cluster`, `root_entries` or `sectors_per_fat`
//! would still let the directory *list* — the root sits at a fixed place
//! either way — and would return wrong bytes. So the assertions go all
//! the way to file contents, as the CP/M ones do.
//!
//! The same disk is here twice, raw and as ImageDisk, which lets one
//! artifact answer two readers: the identical directory must come back
//! through both.

mod common;

use common::{fixtures_dir, open_read};
use remanence::{Format, Session};

fn raw_disk() -> std::path::PathBuf {
    fixtures_dir().join("pcdos-100-disk01.img")
}

fn imd_disk() -> std::path::PathBuf {
    fixtures_dir().join("pcdos-100-disk01.imd")
}

/// The 160 KB disk as raw sectors, declared as the floppy it is.
fn open_raw() -> (Session, remanence::MediaId) {
    let mut session = Session::new();
    let id = session
        .load_media(
            open_read(raw_disk()),
            Format::Raw {
                device: remanence::DeviceType::Floppy(remanence::FloppyDrive::Sector),
                block_bytes: 512,
            },
        )
        .expect("a raw floppy image")
        .id();
    (session, id)
}

/// The names IBM shipped on the 1.00 distribution disk, in the order its
/// own root directory states them.
/// Every name IBM shipped on the 1.00 distribution disk, in the
/// order its own root directory states them — the sixteen commands
/// and the BASIC samples that fill the rest of the volume.
const DISTRIBUTION: [&str; 40] = [
    "IBMBIO.COM",
    "IBMDOS.COM",
    "COMMAND.COM",
    "FORMAT.COM",
    "CHKDSK.COM",
    "SYS.COM",
    "DISKCOPY.COM",
    "DISKCOMP.COM",
    "COMP.COM",
    "DATE.COM",
    "TIME.COM",
    "MODE.COM",
    "EDLIN.COM",
    "DEBUG.COM",
    "LINK.EXE",
    "BASIC.COM",
    "BASICA.COM",
    "ART.BAS",
    "SAMPLES.BAS",
    "MORTGAGE.BAS",
    "COLORBAR.BAS",
    "BUG.BAS",
    "CALENDAR.BAS",
    "MUSIC.BAS",
    "DONKEY.BAS",
    "BLUE.BAS",
    "HUMOR.BAS",
    "POP.BAS",
    "FORTY.BAS",
    "DANDY.BAS",
    "MARCH.BAS",
    "STARS.BAS",
    "HAT.BAS",
    "SCALES.BAS",
    "SAKURA.BAS",
    "CIRCLE.BAS",
    "PIECHART.BAS",
    "SPACE.BAS",
    "BALL.BAS",
    "COMM.BAS",
];

#[test]
fn a_pc_dos_1_volume_reads_without_any_parameter_block() {
    let (mut session, id) = open_raw();
    let medium = session.medium_mut(id).expect("pooled");
    let mut filesystem = medium
        .partition(0)
        .expect("the direct partition")
        .filesystem_as("fat")
        .expect("the media descriptor declares the layout");

    let entries = filesystem.entries("").expect("the root lists");
    let names: Vec<&str> = entries.iter().map(|entry| entry.name.as_str()).collect();
    assert_eq!(names, DISTRIBUTION);
}

#[test]
fn the_files_come_back_at_the_sizes_and_contents_the_disk_states() {
    // The assertion the layout is answerable to. Under a wrong cluster
    // size or root-directory size these still list and come back wrong.
    let (mut session, id) = open_raw();
    let medium = session.medium_mut(id).expect("pooled");
    let mut filesystem = medium
        .partition(0)
        .expect("the direct partition")
        .filesystem_as("fat")
        .expect("the media descriptor declares the layout");

    // An .EXE says what it is in its first two bytes, and this one is
    // large enough — 43,264 bytes — to span many clusters, so reading it
    // whole exercises the chain rather than one entry.
    let link = filesystem.read_file("LINK.EXE").expect("the linker reads");
    assert_eq!(link.len(), 43_264);
    assert_eq!(&link[..2], b"MZ", "an MS-DOS executable header");

    // A .COM is loaded flat at 0x100 and has no header to check, so the
    // check is its exact recorded length and that it is not all zeroes —
    // which is what a mis-walked cluster chain tends to return.
    let command = filesystem
        .read_file("COMMAND.COM")
        .expect("the shell reads");
    assert_eq!(command.len(), 3_231);
    assert!(command.iter().any(|byte| *byte != 0));

    // The two system files IBM writes first, contiguous from cluster 2.
    let bio = filesystem.read_file("IBMBIO.COM").expect("the bios reads");
    assert_eq!(bio.len(), 1_920);
    let dos = filesystem
        .read_file("IBMDOS.COM")
        .expect("the kernel reads");
    assert_eq!(dos.len(), 6_400);
}

#[test]
fn the_same_disk_reads_identically_through_imagedisk() {
    // One artifact, two containers. The ImageDisk one states its sector
    // ids and the raw one has them flattened already, so agreeing here
    // is the ordering ruling (D60) holding on a disk from outside the
    // world it was decided in.
    let (mut raw_session, raw_id) = open_raw();
    let raw_names: Vec<String> = {
        let medium = raw_session.medium_mut(raw_id).expect("pooled");
        let mut filesystem = medium
            .partition(0)
            .expect("the direct partition")
            .filesystem_as("fat")
            .expect("the media descriptor declares the layout");
        filesystem
            .entries("")
            .expect("the root lists")
            .iter()
            .map(|entry| entry.name.clone())
            .collect()
    };

    let mut session = Session::new();
    let id = session
        .load_media(
            open_read(imd_disk()),
            Format::Imd {
                device: remanence::FloppyDrive::Sector,
            },
        )
        .expect("an ImageDisk artifact")
        .id();
    let medium = session.medium_mut(id).expect("pooled");
    let mut filesystem = medium
        .partition(0)
        .expect("the direct partition")
        .filesystem_as("fat")
        .expect("the media descriptor declares the layout here too");

    let names: Vec<String> = filesystem
        .entries("")
        .expect("the root lists")
        .iter()
        .map(|entry| entry.name.clone())
        .collect();
    assert_eq!(names, raw_names);

    // And a file's bytes, not just its name.
    let link = filesystem.read_file("LINK.EXE").expect("the linker reads");
    assert_eq!(link.len(), 43_264);
    assert_eq!(&link[..2], b"MZ");
}
