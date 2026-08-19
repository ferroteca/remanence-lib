// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The CP/M namespace over the Heath distribution disks.
//!
//! **This is the suite that makes the declared layout answerable.** A
//! CP/M volume records nothing about how to read it — the disk parameter
//! block lived in the BIOS — so every number the reader applies is a
//! claim about the world rather than a reading of the artifact. What
//! stops that claim from being unfalsifiable is exactly this: a real
//! Heath distribution disk, and files whose contents say for themselves
//! whether the layout was right.
//!
//! The skew is the reason the file assertions are here and not just a
//! directory listing. A wrong sector map still produces a directory that
//! lists — the first directory sector is where every candidate map
//! agrees — and only the contents come back interleaved. A test that
//! stopped at names would pass against a layout that corrupts every byte
//! it serves.

mod common;

use common::{fixtures_dir, open_read};
use remanence::{ErrorCategory, Format, Session, SpaceRule};

fn distribution_disk() -> std::path::PathBuf {
    fixtures_dir().join("CPM_2_2_02_distribution_1.h8d")
}

/// The H8D adapter declares the H-17 recording, and the disk bears the
/// direct partition; the namespace is the caller's declaration and the
/// adapter's check, as HDOS's is.
fn open_disk(path: std::path::PathBuf) -> (Session, remanence::MediaId) {
    let mut session = Session::new();
    let id = session
        .load_media(open_read(path), Format::H8d)
        .expect("the distribution disk is an H8D")
        .id();
    (session, id)
}

fn open_cpm() -> (Session, remanence::MediaId) {
    open_disk(distribution_disk())
}

#[test]
fn the_declared_layout_lists_the_distribution_disk() {
    let (mut session, id) = open_cpm();
    let medium = session.medium_mut(id).expect("pooled");
    let mut filesystem = medium
        .partition(0)
        .expect("the direct partition")
        .filesystem_as("cpm-heath-h17")
        .expect("the declaration names an enrolled layout");

    let entries = filesystem.entries("").expect("the directory reads");
    let names: Vec<&str> = entries.iter().map(|entry| entry.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "BIOS.SYS",
            "MOVCPM5.COM",
            "MOVCPM8.COM",
            "PIP.COM",
            "SUBMIT.COM",
            "STAT.COM",
            "XSUB.COM",
            "ED.COM",
            "ASM.COM",
            "DDT.COM",
            "LOAD.COM",
            "CONFIGUR.COM",
            "SYSGEN.COM",
            "DUMP.COM",
            "DUMP.ASM",
            "DUP.COM",
            "FORMAT.COM",
        ],
        "the directory the disk states, in the order it states it"
    );

    // CP/M records no byte count: a size is exact to a 128-byte record
    // and no finer, and the reader says the recorded figure rather than
    // guessing where the data stopped.
    let entry = |name: &str| {
        entries
            .iter()
            .find(|entry| entry.name == name)
            .unwrap_or_else(|| panic!("{name} is listed"))
    };
    assert_eq!(entry("BIOS.SYS").size_bytes, 36 * 128);
    assert_eq!(entry("DUMP.COM").size_bytes, 4 * 128);
    assert_eq!(entry("CONFIGUR.COM").size_bytes, 77 * 128);
}

#[test]
fn the_files_read_back_as_what_they_claim_to_be() {
    // This is the assertion the skew is answerable to. Under a wrong
    // sector map both of these still *list*, and both come back as
    // rubbish.
    let (mut session, id) = open_cpm();
    let medium = session.medium_mut(id).expect("pooled");
    let mut filesystem = medium
        .partition(0)
        .expect("the direct partition")
        .filesystem_as("cpm-heath-h17")
        .expect("the declaration names an enrolled layout");

    // An assembler source is its own proof: it is text, and it says what
    // it is on the first line.
    let source = filesystem.read_file("DUMP.ASM").expect("the source reads");
    let head = String::from_utf8_lossy(&source[..64]);
    assert!(
        head.contains("FILE DUMP PROGRAM"),
        "DUMP.ASM opens as its own listing: {head:?}"
    );

    // A transient command is 8080 code loaded at 0x0100. This one opens
    // by loading the stack pointer and carries the copyright of the
    // people who wrote it.
    let command = filesystem.read_file("ASM.COM").expect("the command reads");
    assert_eq!(
        &command[..3],
        &[0x31, 0x00, 0x02],
        "ASM.COM opens with LXI SP,0200h"
    );
    assert!(
        String::from_utf8_lossy(&command[..64]).contains("DIGITAL RESEARCH"),
        "and carries its copyright where the layout says it is"
    );

    // The size the directory states is the size that comes back.
    assert_eq!(command.len(), 64 * 128);
}

#[test]
fn the_account_states_the_layout_and_where_it_came_from() {
    let (mut session, id) = open_cpm();
    let medium = session.medium_mut(id).expect("pooled");
    let mut filesystem = medium
        .partition(0)
        .expect("the direct partition")
        .filesystem_as("cpm-heath-h17")
        .expect("the declaration names an enrolled layout");

    let account = filesystem.evidence().expect("the namespace is present");
    let says = |needle: &str| {
        account.iter().any(|line| line.contains(needle))
            || panic!("the account does not say {needle:?}: {account:?}")
    };

    // What was applied, that the volume did not state it, and that the
    // artifact had to be de-skewed to be read at all.
    says("cpm-heath-h17");
    says("does not record");
    says("skewed");
    // And how the layout came to be believed, which is the part a reader
    // has to be able to weigh (P4) — including that its fields were
    // checked against the block the disks' own BIOS carries, rather than
    // resting on the solve alone.
    says("solved against the CP/M 2.2.02 distribution disk");
    says("disk parameter block those disks carry in their own reserved tracks");
}

#[test]
fn a_later_release_reads_under_the_same_declared_layout() {
    // This is why the enrolled block is named for the drive and not for
    // a release. The 2.2.03 distribution was the second artifact the
    // layout met, and it needed nothing changed — so a release in the
    // identity would have been a distinction the disks do not make.
    let (mut session, id) = open_disk(fixtures_dir().join("CPM_2_2_distribution_1.h8d"));
    let medium = session.medium_mut(id).expect("pooled");
    let mut filesystem = medium
        .partition(0)
        .expect("the direct partition")
        .filesystem_as("cpm-heath-h17")
        .expect("the same layout the earlier release is read by");

    let entries = filesystem.entries("").expect("the directory reads");
    let names: Vec<&str> = entries.iter().map(|entry| entry.name.as_str()).collect();
    assert!(
        names.contains(&"ASSIGN.COM") && names.contains(&"MOVCPM17.COM"),
        "the 2.2.03 system disk's own transients: {names:?}"
    );

    // Contents again, because listing is not the test. This release's
    // DDT is a later build and says so.
    let ddt = filesystem.read_file("DDT.COM").expect("the command reads");
    assert!(
        String::from_utf8_lossy(&ddt[..64]).contains("COPYRIGHT (C) 1980, DIGITAL RESEARCH"),
        "DDT.COM carries its copyright where the layout says it is"
    );
    let asm = filesystem.read_file("ASM.COM").expect("the command reads");
    assert_eq!(&asm[..3], &[0x31, 0x00, 0x02], "ASM.COM opens with LXI SP");
}

#[test]
fn a_file_spanning_four_extents_joins_in_directory_order() {
    // The 2.2.03 distribution's third disk carries a BIOS.ASM long
    // enough to need four directory entries. Joining them is the part of
    // the grammar a synthetic fixture can only approximate, so it is
    // checked here against a real one: the source has to read as one
    // continuous listing, not four shuffled ones.
    let (mut session, id) = open_disk(fixtures_dir().join("CPM_2_2_distribution_3.h8d"));
    let medium = session.medium_mut(id).expect("pooled");
    let mut filesystem = medium
        .partition(0)
        .expect("the direct partition")
        .filesystem_as("cpm-heath-h17")
        .expect("the declaration names an enrolled layout");

    let entries = filesystem.entries("").expect("the directory reads");
    let bios = entries
        .iter()
        .find(|entry| entry.name == "BIOS.ASM")
        .expect("the disk carries one BIOS.ASM");
    assert_eq!(
        bios.declared
            .iter()
            .find(|fact| fact.key == "extents")
            .map(|fact| fact.value.as_str()),
        Some("4"),
        "four directory entries, presented as one file"
    );

    let source = filesystem.read_file("BIOS.ASM").expect("the source reads");
    assert_eq!(source.len(), bios.size_bytes as usize);
    // Text throughout: a mis-joined extent would drop binary or repeated
    // regions into the middle of it.
    let printable = source
        .iter()
        .filter(|byte| byte.is_ascii_graphic() || byte.is_ascii_whitespace() || **byte == 0x1a)
        .count();
    assert_eq!(
        printable,
        source.len(),
        "every byte of the joined source is text"
    );
}

#[test]
fn the_bare_cpm_name_refuses_and_names_the_layouts() {
    // Recognizing a CP/M directory and being able to address it are
    // different claims. The refusal is the honest one, and it points at
    // what a caller can declare instead.
    let (mut session, id) = open_cpm();
    let medium = session.medium_mut(id).expect("pooled");
    let error = medium
        .partition(0)
        .expect("the direct partition")
        .filesystem_as("cpm")
        .expect_err("the layout is not on the disk, so 'cpm' alone cannot read it");

    assert_eq!(error.category(), ErrorCategory::Unsupported);
    assert_eq!(error.rule(), Some(SpaceRule::RecognizedNotRead.as_str()));
    assert!(
        error.to_string().contains("cpm-heath-h17"),
        "the refusal names what to declare instead: {error}"
    );
}

#[test]
fn the_soft_sectored_imagedisk_reads_under_its_own_declared_layout() {
    // D60 in one assertion, and the shape of the answer is worth being
    // exact about. The ImageDisk artifact stores its sectors in the
    // physical interleave and states their ids; the adapter puts them in
    // id order. What remains for the CP/M layout is the *BIOS*
    // translation — and for this soft-sectored recording there is none,
    // because the interleave was written into the sector numbering
    // instead of being applied by the drive.
    //
    // So this reads under a block whose volume parameters are the
    // hard-sectored one's, unchanged, and whose sector map is the
    // identity. Two layouts, differing in exactly the one fact the two
    // recordings differ in.
    let mut session = Session::new();
    let id = session
        .load_media(
            open_read(fixtures_dir().join("cpm_2_2_03_Disk_1.imd")),
            Format::Imd {
                device: remanence::FloppyDrive::HeathH37,
            },
        )
        .expect("the artifact is an ImageDisk")
        .id();
    let medium = session.medium_mut(id).expect("pooled");

    let mut filesystem = medium
        .partition(0)
        .expect("the direct partition")
        .filesystem_as("cpm-heath-soft")
        .expect("the soft-sectored block, whose translation is the identity");

    let entries = filesystem.entries("").expect("the directory reads");
    let names: Vec<&str> = entries.iter().map(|entry| entry.name.as_str()).collect();
    assert!(
        names.contains(&"ASM.COM") && names.contains(&"MOVCPM37.COM"),
        "the soft-sectored release's own transients: {names:?}"
    );

    // Contents, because the directory lists under a wrong ordering too.
    let asm = filesystem.read_file("ASM.COM").expect("the command reads");
    assert_eq!(&asm[..3], &[0x31, 0x00, 0x02], "ASM.COM opens with LXI SP");
    assert!(
        String::from_utf8_lossy(&asm[..64]).contains("DIGITAL RESEARCH"),
        "and carries its copyright where the resolved ordering says it is"
    );
}
