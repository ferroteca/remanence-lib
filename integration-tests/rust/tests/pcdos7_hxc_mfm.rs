// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! An IBM PC DOS 7 distribution disk, read from the HxC MFM container a
//! KryoFlux capture of it was converted to.
//!
//! **This is the artifact the `.mfm` reader was owed.** Synthetic
//! containers prove the reader's arithmetic against a writer that shares
//! its assumptions; this one was written by HxC's own tool, and it
//! settled three things the format's published note does not say: the
//! writer states the bit rate it *measured* (501 here, not 500), it
//! states no RPM at all, and it keeps reading past the index it started
//! from so a track holds a little more than one revolution. Each is
//! asserted below as the evidence the medium declares for it.
//!
//! The payoff is the ladder end to end on a PC disk: cells to bits to
//! MFM bytes to the recording's own sectors, and a FAT volume opening
//! through the ordinary partition seam above them — with file contents
//! checked, because a directory lists under a wrong ordering too.

mod common;

use common::{fixtures_dir, open_read};
use remanence::{DeviceType, FloppyDrive, Format, MediaId, Session};

fn load() -> (Session, MediaId) {
    let mut session = Session::new();
    let id = session
        .load_media(
            open_read(fixtures_dir().join("pcdos-7-de-disk1.mfm")),
            Format::HxcMfm {
                device: FloppyDrive::Pc35Hd,
            },
        )
        .expect("the container loads against the PC high-density family")
        .id();
    (session, id)
}

#[test]
fn the_container_declares_what_hxc_actually_writes() {
    let (mut session, id) = load();
    let medium = session.medium_mut(id).expect("pooled");
    assert_eq!(
        medium.device_type(),
        Some(DeviceType::Floppy(FloppyDrive::Pc35Hd))
    );
    assert_eq!(medium.article(), "flexible-3.5-hd");

    let evidence = medium.assurance().evidence.join("\n");
    let says = |fragment: &str| {
        assert!(
            evidence.contains(fragment),
            "{fragment:?} is not in:\n{evidence}"
        );
    };
    // Eighty cylinders plus the four the capture drive kept stepping to.
    says("an HxC MFM container of 84 track(s) by 2 side(s)");
    // The three things learned from the writer rather than the note.
    says("stating 501 kbit/s at 0 RPM");
    says("states 501 kbit/s, a measured figure within one part in 50 of the family's 500 kbit/s");
    says(
        "states no RPM, which is how the HxC writer leaves the field; the family's 300 RPM is taken",
    );
    says("state more cells than one revolution holds");
    says("declares loss 'hxc-mfm.track-longer-than-the-circle'");
    // And what the container does not carry, stated rather than supplied.
    says("no weak region, no density variation and no second observation");
    says("declared synthetic rather than presented as recovered timing");
}

#[test]
fn the_ladder_reads_every_sector_and_the_fat_volume_opens_above_them() {
    let (mut session, id) = load();
    let medium = session.medium_mut(id).expect("pooled");

    // Bits: every location inside the family's eighty cylinders clocks
    // at the 16-cycle cell a megahertz of cells is, and the four
    // cylinders past the density zone are left out rather than clocked
    // at a rate the family never declared for them.
    let bits = medium.bitstream().expect("the channel clocks it").inspect();
    assert_eq!(bits.profile_id, "pc-3.5-hd");
    assert_eq!(bits.locations.len(), 160);
    assert!(
        bits.locations
            .iter()
            .all(|location| location.zone == 0 && location.cell_cycles_numerator == 16),
        "{:?}",
        bits.locations.first()
    );

    // Bytes, framed at the marks the MFM codec located.
    let bytes = medium.bytestream().expect("the codec resolves it");
    let framed = bytes.inspect();
    assert_eq!(framed.codec_id, "ibm-mfm");
    assert_eq!(framed.locations.len(), 160);
    for location in &framed.locations {
        // Eighteen records, each an id field and a data field, plus the
        // index mark: the marks a high-density track carries.
        assert!(location.alignments >= 36, "{location:?}");
        assert!(location.bytes > 12_000, "{location:?}");
    }

    // Sectors: the recording's own, every one of them readable.
    let sectors = bytes
        .recognize_sectors(1 << 22)
        .expect("the family's record grammar reads the recording's own sectors")
        .into_ibm()
        .expect("an IBM recording answers the IBM reading");
    let report = sectors.inspect();
    assert_eq!(report.claims.len(), 80 * 2 * 18);
    assert!(report.claims.iter().all(|claim| claim.readable()));
    assert!(report.claims.iter().all(|claim| !claim.data_deleted));
    let geometry = sectors
        .geometry()
        .expect("the records compose a uniform image");
    assert_eq!(
        (
            geometry.cylinders,
            geometry.heads,
            geometry.sectors_per_track,
            geometry.first_sector,
            geometry.sector_bytes
        ),
        (80, 2, 18, 1, 512)
    );
    assert_eq!(geometry.length_bytes(), 1_474_560);

    // The namespace door: the extent those sectors compose, opened the
    // way a hard-disk image's partition is (D62), with no flux
    // vocabulary reaching the FAT adapter.
    let mut partition = sectors.partition().expect("the records compose an extent");
    let mut volume = partition
        .view()
        .filesystem_as("fat")
        .expect("a FAT volume is declared and the content bears it");

    let label = volume
        .label()
        .expect("the namespace answers")
        .expect("IBM labelled the distribution disks");
    assert_eq!(label.name.as_deref(), Some("DISK      1"));

    // The root in the order IBM wrote it: the system files first, as a
    // bootable DOS disk must have them.
    let entries = volume.entries("").expect("the root lists");
    let names: Vec<&str> = entries.iter().map(|entry| entry.name.as_str()).collect();
    assert_eq!(names.len(), 44, "{names:?}");
    assert_eq!(&names[..3], ["IBMBIO.COM", "IBMDOS.COM", "COMMAND.COM"]);
    assert!(names.contains(&"README.TXT"), "{names:?}");

    // Contents, not only names. IBMBIO.COM opens with a jump, and a file
    // this size spans many clusters, so this walks the chain rather than
    // one entry; COMMAND.COM has no header to check, so the check is its
    // recorded length and that it is not the zeroes a mis-walked chain
    // returns.
    let bio = volume.read_file("IBMBIO.COM").expect("the BIOS reads");
    assert_eq!(bio.len(), 40_863);
    assert_eq!(&bio[..3], &[0xe9, 0x35, 0x01]);
    let command = volume.read_file("COMMAND.COM").expect("the shell reads");
    assert_eq!(command.len(), 55_377);
    assert!(command.iter().any(|byte| *byte != 0));
}
