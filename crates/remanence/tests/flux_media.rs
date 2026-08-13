// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The media-first flux journey (F59): a KryoFlux capture loads as a
//! medium through the same verb every other medium arrives through,
//! and everything the recording can answer answers on it.
//!
//! The walk is U26's with U25's read and U33's independence folded in:
//! the archive is a medium, its content is a namespace, the capture is
//! a collection gathered from that namespace and declared — member
//! grammar, completeness, stream grammar and the profile claim checked
//! whole, then the reduction under the profile's declared
//! materialization defaults — and what comes back is a 1541 disk with
//! the whole story as provenance. The disk outlives its source, enters
//! a machine of its own, and reads through the argument-free
//! presentation rungs (P30 reached through the type): the bitstream,
//! the bytestream and its framed bytes, the recording's own sectors,
//! and the directory CBM DOS wrote across them — every number below
//! what a 1541 makes of an actual recording rather than what a
//! synthetic one was built to produce.
//!
//! The cheap refusals sit beside the journey: a shape the format does
//! not read, a collection that does not hold together, and the flux
//! questions asked of a family that bears no flux — each refused by
//! name before anything expensive runs.

use std::fs::File;

use remanence::{
    DeviceType, ErrorCategory, FloppyDrive, Format, Location, MediaId, SectorRule, Session,
};

mod common;

const ARCHIVE: &str = "Bill Budge Pinball Construction Set [Commodore 64] (1of2).7z";
/// 84 drive-step positions, each captured by both heads.
const MEMBER_COUNT: usize = 168;
/// The recordings the reduction admits from a whole side, the fat
/// track merged rather than asserted.
const LOCATIONS: usize = 36;
/// The 35-track grid CBM DOS defines, as the family declares it: the
/// four zones with their track boundaries and sector counts.
const ZONES: [(u8, u8, u8); 4] = [(1, 17, 21), (18, 24, 19), (25, 30, 18), (31, 35, 17)];

/// The whole journey runs in one test, deliberately: the reduction
/// costs minutes, so the disk is mastered once and every claim below
/// reads the same medium.
#[test]
fn a_capture_loads_as_a_medium_and_the_whole_ladder_answers() {
    let path = std::env::temp_dir().join(format!("remanence-flux-media-{}.7z", std::process::id()));
    std::fs::copy(common::ensure_fixture(ARCHIVE), &path).expect("fixture copies");

    let mut session = Session::new();

    // 1. The archive is a medium, and this is what that call answers
    //    with (U26).
    let arc_id = session
        .load_media(
            File::open(&path).expect("the fixture opens"),
            Format::SevenZip,
        )
        .expect("the archive loads")
        .id();

    // 2. Its content is a namespace; the capture is the collection
    //    gathered from it and declared. The sources are free-standing,
    //    so the walk ends before the load begins.
    let members = {
        let arc = session.medium_mut(arc_id).expect("the archive is pooled");
        arc.partition(0)
            .expect("an archive bears its direct partition")
            .filesystem()
            .expect("an archive's content is its namespace")
            .files("")
            .expect("the members gather")
    };
    assert_eq!(members.len(), MEMBER_COUNT);

    // 3. What comes back is a disk, reached the way every medium is —
    //    not a root peculiar to captures.
    let disk_id = session
        .load_media(
            members,
            Format::KryoFlux {
                device: FloppyDrive::Commodore1541,
            },
        )
        .expect("the declared collection masters a disk")
        .id();

    medium_facts(&mut session, disk_id);
    provenance_is_the_whole_story(&mut session, disk_id);
    the_ladder_answers_argument_free(&mut session, disk_id);
    sectors_and_directory(&mut session, disk_id);
    the_disk_outlives_its_source(&mut session, arc_id, disk_id);

    session.release_media(disk_id).expect("the disk releases");
    std::fs::remove_file(&path).ok();
}

/// The medium says what it is: the declared family, the article the
/// profile declares it is served on, and the direct partition a
/// recording bears.
fn medium_facts(session: &mut Session, disk_id: MediaId) {
    let disk = session.medium_mut(disk_id).expect("the disk is pooled");

    assert_eq!(
        disk.device_type(),
        Some(DeviceType::Floppy(FloppyDrive::Commodore1541))
    );
    assert_eq!(disk.article(), "flexible-5.25-soft");
    assert!(!disk.is_modified());
    assert!(
        disk.image_size_bytes() > 1_000_000,
        "the raw plane's extent"
    );

    // A recording records no scheme: the direct partition, extent-less,
    // its namespace declared rather than determined.
    assert_eq!(disk.partition_scheme(), None);
    let partitions = disk.partitions();
    assert_eq!(partitions.len(), 1);
    assert!(partitions[0].is_direct());
    assert_eq!(partitions[0].start_bytes(), None);
    assert!(!partitions[0].is_addressable());
    assert!(!partitions[0].bears_namespace());
    let view = disk.partition(0).expect("the direct partition");
    assert!(view.volume().is_none(), "no extent composes a volume");
    assert!(
        disk.partition(0)
            .expect("the direct partition")
            .filesystem()
            .is_none(),
        "nothing determines a namespace, and this layer will not pick one"
    );

    // The identification states the layers: the collection format and
    // the article the family is served.
    let identification = disk.identify();
    let ids: Vec<&str> = identification
        .layers
        .iter()
        .map(|layer| layer.id.as_str())
        .collect();
    assert_eq!(ids, ["kryoflux", "flexible-5.25-soft"]);

    // The byte-plane and space verbs refuse by name: a flux medium's
    // evidence stays behind the surface (P13).
    let mut buf = [0u8; 4];
    let error = disk.read_at(0, &mut buf).expect_err("no byte plane");
    assert!(error.to_string().contains("presentation ladder"), "{error}");
    assert!(disk.format().is_err(), "no block image format");
    assert!(disk.commit().is_err(), "nothing to commit");
    let error = disk
        .read_sector(0, 0, 1, &mut [0u8; 256])
        .expect_err("no cylinder-head-sector coordinates are stated");
    assert!(error.to_string().contains("geometry"), "{error}");
}

/// The verdicts, the policy and the declared-loss account ride the
/// medium as provenance (P29): the whole story, before anything else is
/// asked.
fn provenance_is_the_whole_story(session: &mut Session, disk_id: MediaId) {
    let disk = session.medium(disk_id).expect("the disk is pooled");
    let assurance = disk.assurance();

    assert_eq!(assurance.access, remanence::AccessMode::ReadOnly);
    let says = |fragment: &str| {
        assert!(
            assurance
                .evidence
                .iter()
                .any(|line| line.contains(fragment)),
            "{fragment:?} is not in {:?}",
            assurance.evidence
        );
    };
    // The member grammar and its completeness.
    says("168 KryoFlux stream members");
    // The profile claim, checked whole, and the side measured.
    says("the 'c1541' claim checked whole and borne");
    says("measured on capture head 0");
    // The reduction under the profile's declared defaults.
    says("declared materialization defaults");
    says("count-spread discriminator");
    // The declared-loss account, entry by entry — a count is not an
    // account.
    says("declares loss");
    // The served projection's own account.
    says("the served projection");
}

/// The argument-free rungs (P30 reached through the type): the type
/// carries the channel and the codec, and the same state answers every
/// call.
fn the_ladder_answers_argument_free(session: &mut Session, disk_id: MediaId) {
    let disk = session.medium_mut(disk_id).expect("the disk is pooled");

    let report = disk.bitstream().expect("the channel clocks it").inspect();
    assert_eq!(report.profile_id, "c1541");
    assert_eq!(report.reference_clock_hz, 16_000_000);
    assert_eq!(report.cycles_per_rotation, 3_200_000);
    assert_eq!(report.locations.len(), LOCATIONS);
    // The four documented zones at their documented cells: each location
    // clocked at the rate its own zone declares.
    let cells: Vec<(u32, u64)> = {
        let mut cells: Vec<(u32, u64)> = report
            .locations
            .iter()
            .map(|location| (location.zone, location.cell_cycles_numerator))
            .collect();
        cells.sort_unstable();
        cells.dedup();
        cells
    };
    assert_eq!(cells, [(0, 52), (1, 56), (2, 60), (3, 64)]);
    let report = report.clone();

    let bytestream = disk.bytestream().expect("the codec resolves it");
    let inspected = bytestream.inspect();
    assert_eq!(inspected.codec_id, "c1541-gcr");
    assert_eq!(inspected.symbols_per_byte, 2);
    assert_eq!(inspected.locations.len(), LOCATIONS);
    for location in &inspected.locations {
        assert!(location.alignments >= 30, "{location:?}");
        assert!(location.bytes > 4_000, "{location:?}");
        // Most of the recording resolves through the family's own table.
        assert!(
            location.resolved_bytes * 10 > location.bytes * 9,
            "{location:?}"
        );
    }

    // U25's read: the framed bytes of one location, addressed the
    // family's own way. The byte is the first *framed* byte, because
    // nothing before sync is a byte at all — and on a CBM disk the
    // framing lands on a block mark.
    let track_one = bytestream
        .location(Location::track(1))
        .expect("the disk holds track 1");
    assert!(track_one.bytes() > 4_000);
    let mut first = [0u8; 1];
    track_one
        .read_at(0, &mut first)
        .expect("the first framed byte reads");
    assert!(
        first[0] == 0x07 || first[0] == 0x08,
        "framing begins at a sync, and a sync introduces a block mark: {:#04x}",
        first[0]
    );
    // A track the family's addressing does not reach is absent rather
    // than blank.
    let error = bytestream
        .location(Location::track(36))
        .err()
        .expect("there is no track 36");
    assert_eq!(error.category(), ErrorCategory::NotFound);
    // A read past the location's framed bytes is refused whole.
    let held = track_one.bytes();
    let mut past = [0u8; 2];
    assert!(track_one.read_at(held - 1, &mut past).is_err());

    // Materialized once: asking again answers the same state rather
    // than running the transition twice.
    let again = disk.bitstream().expect("the same state answers").inspect();
    assert_eq!(*again, report);
}

/// The recording's own sectors, recognized through the public rung, and
/// the directory CBM DOS wrote across them — read through the medium's
/// own namespace door.
fn sectors_and_directory(session: &mut Session, disk_id: MediaId) {
    let disk = session.medium_mut(disk_id).expect("the disk is pooled");

    // The third rung is reached from the second, argument-free.
    let sectors = disk
        .bytestream()
        .expect("the codec resolves it")
        .recognize_sectors(1 << 20)
        .expect("the family's record grammar reads the recording's own sectors");
    let report = sectors.inspect();
    assert_eq!(report.grammar_id, "cbm-dos-record");
    assert!(report.claims.len() > 600, "{} claims", report.claims.len());

    // The disk's own block-availability map, read out of flux through
    // four layers: it links to track 18 sector 1 and states CBM DOS
    // version 'A'.
    let bam = sectors.read_sector(18, 0).expect("track 18 sector 0 reads");
    assert_eq!(&bam[..3], &[18, 1, 0x41]);

    // Most of the 683-block grid comes back, and every address that
    // does not is a refusal naming its rule rather than a block of
    // zeros.
    let mut read = 0u32;
    for (first, last, sectors_on) in ZONES {
        for track in first..=last {
            for sector in 0..sectors_on {
                match sectors.read_sector(track, sector) {
                    Ok(payload) => {
                        assert_eq!(payload.len(), 256);
                        read += 1;
                    }
                    Err(error) => {
                        let rule = error.rule().expect("a sector refusal names its rule");
                        assert!(SectorRule::from_identity(rule).is_some(), "{rule}");
                    }
                }
            }
        }
    }
    assert!(read > 640, "{read} of the grid's 683 blocks read");

    // The namespace door is the medium's own: the direct partition,
    // with the reading declared and checked (U26).
    let mut cbm = disk
        .partition(0)
        .expect("flux media record no scheme: the direct partition")
        .filesystem_as("cbmdos")
        .expect("this disk's directory track reads");
    assert_eq!(cbm.kind().expect("a namespace was recognized"), "cbmdos");
    assert!(!cbm.is_addressable());

    // The BAM header as the label — this disk's name is the autoboot
    // trick, and its unreadable control bytes are marked in the reading
    // and carried whole beside it.
    let label = cbm
        .label()
        .expect("the namespace answers")
        .expect("a CBM DOS disk names itself");
    assert_eq!(label.answered_by.as_deref(), Some("bam-disk-name"));

    // The recorded directory in directory order — the order is
    // evidence — with the facts CBM DOS records.
    let entries = cbm.entries("").expect("the directory chain reads");
    let names: Vec<&str> = entries.iter().map(|entry| entry.name.as_str()).collect();
    assert_eq!(names.len(), 16, "{names:?}");
    assert_eq!(names[0], "EA");
    assert_eq!(names[3], "DEMO1.PB");
    let program = entries
        .iter()
        .find(|entry| entry.name == "PCS.4000")
        .expect("the disk holds it");
    assert_eq!(program.size_bytes, 64 * 254 + 130);
    let bytes = cbm.read_file("PCS.4000").expect("the chain reads");
    assert_eq!(bytes.len() as u64, program.size_bytes);
    // A Commodore program's first two bytes are the address it loads
    // at. Four layers down that is a flux transition, so the whole
    // ladder is standing.
    assert_eq!(&bytes[..2], &[0x00, 0x40]);

    // Read-only in this release, said by name.
    let error = cbm
        .write_file("EA", b"no")
        .expect_err("this release does not write CBM DOS");
    assert_eq!(error.category(), ErrorCategory::ReadOnly);
    drop(cbm);

    // A namespace with an extent-less door: declaring a reading that
    // needs an addressed extent is refused naming what is missing.
    let error = disk
        .partition(0)
        .expect("the direct partition")
        .filesystem_as("fat")
        .expect_err("a recording composes no addressed extent for FAT");
    assert!(error.to_string().contains("no addressed extent"), "{error}");
}

/// U33: media are session state, independent of every machine and of
/// each other — the archive a disk was mastered out of is not the
/// disk's parent.
fn the_disk_outlives_its_source(session: &mut Session, arc_id: MediaId, disk_id: MediaId) {
    session
        .release_media(arc_id)
        .expect("the source archive leaves the session");

    // The mastered disk is free-standing and still answers.
    let disk = session.medium_mut(disk_id).expect("the disk is pooled");
    let mut byte = [0u8; 1];
    disk.bytestream()
        .expect("the presentation still answers")
        .location(Location::track(1))
        .expect("the track is still there")
        .read_at(0, &mut byte)
        .expect("and still reads");

    // It enters a machine of its own, and leaves it untouched.
    let mut c64 = session.add_machine("c64").expect("the machine is added");
    let drive = c64
        .add_device(FloppyDrive::Commodore1541)
        .expect("the drive an emulator will one day address as unit 8")
        .attachment();
    session
        .machine_mut("c64")
        .expect("just added")
        .device_mut(drive)
        .expect("just added")
        .insert(disk_id)
        .expect("device-type equality admits the disk");
    assert!(session.medium(disk_id).expect("pooled").is_linked());
    session
        .machine_mut("c64")
        .expect("still here")
        .device_mut(drive)
        .expect("still here")
        .eject()
        .expect("sever — claim and state survive pooled");
    session.release_machine("c64").expect("the cascade");
    assert!(!session.medium(disk_id).expect("pooled").is_linked());
}

// --------------------------------------------------- the cheap refusals

/// A format declares which source shape it reads, and a shape it does
/// not read is refused by name before anything else runs.
#[test]
fn a_shape_the_format_does_not_read_is_refused_by_name() {
    let mut session = Session::new();
    let loose = std::env::temp_dir().join(format!("remanence-shape-{}.raw", std::process::id()));
    std::fs::write(&loose, b"").expect("the scratch file writes");

    // One artifact offered to the collection-sourced format.
    let error = session
        .load_media(
            File::open(&loose).expect("it opens"),
            Format::KryoFlux {
                device: FloppyDrive::Commodore1541,
            },
        )
        .expect_err("a capture set is a collection");
    assert!(error.to_string().contains("declared collection"), "{error}");
    assert!(error.to_string().contains("one opened file"), "{error}");

    // A collection offered to a one-artifact format.
    let error = session
        .load_media(
            vec![File::open(&loose).expect("it opens")],
            Format::SevenZip,
        )
        .expect_err("an archive is one artifact");
    assert!(error.to_string().contains("one artifact"), "{error}");
    assert!(
        error.to_string().contains("collection of 1 opened files"),
        "{error}"
    );

    std::fs::remove_file(&loose).ok();
}

/// The member grammar and the set's completeness refuse the whole
/// declaration by name, before any stream is decoded (U25's "checked,
/// not trusted").
#[test]
fn a_collection_that_does_not_hold_together_is_refused_whole() {
    let dir = std::env::temp_dir().join(format!("remanence-members-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("the scratch directory");
    for name in ["cap00.0.raw", "cap02.0.raw"] {
        std::fs::write(dir.join(name), b"").expect("the member writes");
    }

    let mut session = Session::new();
    let members: Vec<File> = ["cap00.0.raw", "cap02.0.raw"]
        .iter()
        .map(|name| File::open(dir.join(name)).expect("the member opens"))
        .collect();
    let error = session
        .load_media(
            members,
            Format::KryoFlux {
                device: FloppyDrive::Commodore1541,
            },
        )
        .expect_err("a hole in the set refuses the whole set");
    assert_eq!(error.category(), ErrorCategory::InvalidImage);
    assert!(
        error
            .to_string()
            .contains("step position 1 head 0 is absent"),
        "{error}"
    );

    std::fs::remove_dir_all(&dir).ok();
}

/// The pairing within the class is the catalog's check: a floppy the
/// release records no capture of refuses at the declaration (P3, P14).
#[test]
fn a_pairing_no_adapter_declares_is_refused_at_the_declaration() {
    let mut session = Session::new();
    let error = session
        .load_media(
            Vec::<File>::new(),
            Format::KryoFlux {
                device: FloppyDrive::HeathH17,
            },
        )
        .expect_err("the release records no KryoFlux reading of an H-17");
    assert!(error.to_string().contains("h17"), "{error}");
    assert!(error.to_string().contains("c1541"), "{error}");
}

/// The flux questions answer where the device type's profile bears
/// flux, and refuse by name everywhere else (P13, P30).
#[test]
fn a_medium_whose_family_bears_no_flux_refuses_the_flux_questions() {
    let path =
        std::env::temp_dir().join(format!("remanence-flux-refusal-{}.7z", std::process::id()));
    std::fs::copy(common::ensure_fixture(ARCHIVE), &path).expect("fixture copies");

    let mut session = Session::new();
    let arc = session
        .load_media(
            File::open(&path).expect("the fixture opens"),
            Format::SevenZip,
        )
        .expect("the archive loads");
    let error = arc
        .bitstream()
        .expect_err("an archive was recorded by no device");
    assert!(error.to_string().contains("archive medium"), "{error}");
    assert!(error.to_string().contains("bears flux"), "{error}");

    let arc_id = arc.id();
    session.release_media(arc_id).expect("released");
    std::fs::remove_file(&path).ok();
}
