// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! Discovered geometry and the recording's own coordinates, over
//! synthetic images the project owns outright.
//!
//! The two readings that need an artifact of their own — a format that
//! declares a geometry, and one nested in an archive — are in
//! `integration-tests/rust/tests/geometry_fixtures.rs`, behind the
//! `fixtures` feature.
//!
//! Every geometry here is *read*: nothing in these tests declares one
//! onto a medium, because nothing can. What the tests vary is what the
//! artifact says about itself — a format that declares a geometry for
//! every image it claims, a boot record that recorded the drive it was
//! formatted on, a partition table whose end tuples imply one, and the
//! extent the content actually has — and then what the library does when
//! two of those disagree, which is to report both and settle neither.

use std::path::PathBuf;

use remanence::{
    ErrorCategory, Format, GeometrySource, GeometryState, HardDrive, MediaId, RecordingGeometry,
    Session,
};

mod common;
use common::{open_read, open_write};

/// The rule identities this seam's refusals carry. They are the
/// caller-facing half of "refuse by name", so the tests name them the
/// way a caller would branch on them.
const NOT_SECTOR_ADDRESSED: &str = "not-sector-addressed";
const GEOMETRY_UNSTATED: &str = "geometry-unstated";
const GEOMETRY_UNDETERMINED: &str = "geometry-undetermined";
const OUTSIDE_GEOMETRY: &str = "outside-geometry";
const PARTIAL_SECTOR: &str = "partial-sector";

fn temp_path(tag: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nonce = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "remanence-geometry-{tag}-{}-{nonce}.img",
        std::process::id()
    ))
}

/// A minimal FAT16 volume whose boot record records the track geometry
/// it was formatted under — the fields DOS wrote down and this library
/// reads back as one source among several.
fn fat16_volume(sectors_per_track: u16, heads: u16, total_sectors: usize) -> Vec<u8> {
    let mut image = vec![0u8; total_sectors * 512];

    image[0] = 0xeb;
    image[1] = 0x3c;
    image[2] = 0x90;
    image[3..11].copy_from_slice(b"REMANENC");
    image[11..13].copy_from_slice(&512u16.to_le_bytes());
    image[13] = 1; // sectors/cluster
    image[14..16].copy_from_slice(&1u16.to_le_bytes()); // reserved
    image[16] = 2; // FATs
    image[17..19].copy_from_slice(&512u16.to_le_bytes()); // root entries
    image[19..21].copy_from_slice(&(total_sectors as u16).to_le_bytes());
    image[21] = 0xf8;
    image[22..24].copy_from_slice(&32u16.to_le_bytes()); // sectors/FAT
    image[24..26].copy_from_slice(&sectors_per_track.to_le_bytes());
    image[26..28].copy_from_slice(&heads.to_le_bytes());
    image[510] = 0x55;
    image[511] = 0xaa;

    for fat in 0..2usize {
        let base = (1 + fat * 32) * 512;
        image[base..base + 2].copy_from_slice(&0xfff8u16.to_le_bytes());
        image[base + 2..base + 4].copy_from_slice(&0xffffu16.to_le_bytes());
    }
    image
}

/// One block in the packed cylinder-head-sector form an MBR slot
/// records, under the geometry the machine that wrote it used.
fn chs(block: u64, heads: u64, sectors_per_track: u64) -> [u8; 3] {
    let per_cylinder = heads * sectors_per_track;
    let cylinder = block / per_cylinder;
    let head = (block % per_cylinder) / sectors_per_track;
    let sector = block % sectors_per_track + 1;
    [
        head as u8,
        (sector as u8 & 0x3f) | (((cylinder >> 2) as u8) & 0xc0),
        (cylinder & 0xff) as u8,
    ]
}

/// One volume behind a one-entry MBR starting at `start`, the entry's
/// end tuple written under `tuple` where a machine wrote one and left
/// zero where none did.
fn mbr_disk(volume: &[u8], start: usize, tuple: Option<(u64, u64)>) -> Vec<u8> {
    let sectors = volume.len() / 512;
    let mut disk = vec![0u8; (start + sectors) * 512];
    let last = (start + sectors - 1) as u64;

    disk[450] = 0x06; // FAT16B
    if let Some((heads, sectors_per_track)) = tuple {
        disk[451..454].copy_from_slice(&chs(last, heads, sectors_per_track));
    }
    disk[454..458].copy_from_slice(&(start as u32).to_le_bytes());
    disk[458..462].copy_from_slice(&(sectors as u32).to_le_bytes());
    disk[510] = 0x55;
    disk[511] = 0xaa;
    disk[start * 512..start * 512 + volume.len()].copy_from_slice(volume);
    disk
}

/// The disk a machine that addressed its drive by cylinder, head and
/// sector wrote: the table at block zero, the volume starting at head 1
/// of cylinder 0 (block 18), and both ending on a cylinder boundary of
/// the 2 heads of 18 sectors it used — 222 cylinders in all.
///
/// The layout matters. An end tuple names one block in coordinates and
/// the extent names the same block by number, and it is the pair that
/// states a geometry; a partition laid out the way a later, block-
/// addressing machine lays one out leaves the pair solvable more than
/// one way, and then the tuple states nothing at all.
fn chs_disk(volume: &[u8]) -> Vec<u8> {
    mbr_disk(volume, CHS_START, Some((2, 18)))
}

/// The 7,974-sector volume that layout carries.
fn chs_volume(sectors_per_track: u16, heads: u16) -> Vec<u8> {
    fat16_volume(sectors_per_track, heads, 7_974)
}

const CHS_START: usize = 18;

/// A later machine's layout over the same drive geometry: the volume at
/// block 2,048, ending where it ends. Nothing about it is
/// cylinder-aligned, which is why its own end tuple would state nothing
/// and its boot record is left to.
fn lba_disk() -> Vec<u8> {
    mbr_disk(&fat16_volume(18, 2, 8_000), 2_048, None)
}

/// Pools an artifact under a declared format and answers with both,
/// because a medium lives in its session's pool.
fn pool(source: std::fs::File, format: Format) -> (Session, MediaId) {
    let mut session = Session::new();
    let id = session
        .load_media(source, format)
        .expect("the declaration is borne")
        .id();
    (session, id)
}

fn raw_sector_hd() -> Format {
    Format::Raw {
        device: HardDrive::MbrSector.into(),
        block_bytes: 512,
    }
}

/// What the CHS-era disk states: 7,992 blocks of 512 under 2 heads of 18
/// sectors, which is exactly 222 cylinders.
fn chs_geometry() -> RecordingGeometry {
    RecordingGeometry {
        cylinders: 222,
        heads: 2,
        sectors_per_track: 18,
        sector_bytes: 512,
    }
}

/// What the later layout states: the same track geometry over 10,048
/// blocks, which reaches into a 280th cylinder the content does not
/// wholly hold.
fn lba_geometry() -> RecordingGeometry {
    RecordingGeometry {
        cylinders: 280,
        heads: 2,
        sectors_per_track: 18,
        sector_bytes: 512,
    }
}

#[test]
fn agreeing_sources_establish_the_coordinates_and_each_reading_says_where_it_came_from() {
    let path = temp_path("agreeing");
    std::fs::write(&path, chs_disk(&chs_volume(18, 2))).expect("writes");

    let (session, id) = pool(open_read(&path), raw_sector_hd());
    let medium = session.medium(id).expect("pooled");
    let geometry = medium.geometry();

    assert_eq!(geometry.state(), GeometryState::Determined);
    assert_eq!(geometry.determined(), Some(chs_geometry()));
    assert!(geometry.conflicts().is_empty());
    assert!(geometry.unsettled().is_empty());

    // Every source a load reads is on the record, with where it was read.
    // Authorship is deliberately not among them: it is the one source
    // that is no reading of an artifact, and it belongs to the one medium
    // that has none — an authored geometry never stands beside another.
    let sources: Vec<GeometrySource> = geometry
        .readings()
        .iter()
        .map(|reading| reading.source)
        .collect();
    for source in GeometrySource::ALL {
        // The two authorship-side sources never appear here. Nothing is
        // authored onto a medium that was loaded, and nothing is
        // recorded onto one either — a loaded disk already testifies for
        // itself, which is what the other four readings are.
        if matches!(
            source,
            GeometrySource::Authorship | GeometrySource::Recording
        ) {
            assert!(
                !sources.contains(&source),
                "{source} is an author's fact and this medium was loaded: {sources:?}"
            );
            continue;
        }
        assert!(
            sources.contains(&source),
            "{source} states something about this disk and is not on the \
             record: {sources:?}"
        );
    }
    let boot = geometry
        .readings()
        .iter()
        .find(|reading| reading.source == GeometrySource::BootRecord)
        .expect("the volume's boot record recorded its track geometry");
    assert_eq!(boot.heads, Some(2));
    assert_eq!(boot.sectors_per_track, Some(18));
    assert_eq!(boot.sector_bytes, Some(512));
    assert_eq!(
        boot.cylinders, None,
        "a boot record states no cylinder count: its own sector total \
         describes the volume, not the drive"
    );
    assert!(
        boot.at.contains("partition 1"),
        "the reading names where it was taken: {}",
        boot.at
    );

    let tuple = geometry
        .readings()
        .iter()
        .find(|reading| reading.source == GeometrySource::PartitionTable && reading.heads.is_some())
        .expect("the end tuple implies one");
    assert_eq!(
        (tuple.heads, tuple.sectors_per_track),
        (Some(2), Some(18)),
        "the tuple was written under the same geometry the boot record was"
    );

    drop(session);
    std::fs::remove_file(&path).ok();
}

#[test]
fn the_coordinates_address_the_recording_and_a_write_buffers_until_commit() {
    let path = temp_path("coordinates");
    let disk = chs_disk(&chs_volume(18, 2));
    std::fs::write(&path, &disk).expect("writes");

    let (mut session, id) = pool(open_write(&path), raw_sector_hd());
    let medium = session.medium_mut(id).expect("pooled");

    // Cylinder 0, head 0, sector 1 is block zero — the table itself.
    let mut leading = [0u8; 512];
    medium
        .read_sector(0, 0, 1, &mut leading)
        .expect("the first sector reads");
    assert_eq!(&leading[510..], &[0x55, 0xaa], "the boot signature");

    // Block 18 under 2 heads of 18 sectors is cylinder 0, head 1,
    // sector 1 — the volume's own boot record, reached in the
    // recording's coordinates rather than by an offset computed by hand.
    let mut boot = [0u8; 512];
    medium
        .read_sector(0, 1, 1, &mut boot)
        .expect("the volume's boot record reads");
    assert_eq!(&boot[3..11], b"REMANENC", "the BPB's own OEM name");

    // A write is buffered like every other write (P2): the session sees
    // it, the file does not, and commit is what moves it.
    let mut written = [0x5au8; 512];
    written[0] = 0xc0;
    medium
        .write_sector(221, 1, 18, &written)
        .expect("the last block writes");
    let mut read_back = [0u8; 512];
    medium
        .read_sector(221, 1, 18, &mut read_back)
        .expect("reads back through the session");
    assert_eq!(read_back, written, "the session reads its own write");
    assert!(medium.is_modified());
    assert_eq!(
        std::fs::read(&path).expect("reads")[disk.len() - 512..],
        disk[disk.len() - 512..],
        "nothing reached the file before the commit"
    );

    medium.commit().expect("commits");
    assert_eq!(
        &std::fs::read(&path).expect("reads")[disk.len() - 512..],
        &written[..],
        "the committed sector is in the file"
    );

    // And a rollback discards a later one, leaving the committed state.
    let medium = session.medium_mut(id).expect("pooled");
    medium
        .write_sector(221, 1, 18, &[0u8; 512])
        .expect("writes again");
    medium.rollback().expect("rolls back");
    medium
        .read_sector(221, 1, 18, &mut read_back)
        .expect("reads back");
    assert_eq!(read_back, written, "the rollback left the committed bytes");

    drop(session);
    std::fs::remove_file(&path).ok();
}

#[test]
fn a_coordinate_the_geometry_or_the_content_does_not_hold_is_refused_by_name() {
    // The later layout, whose extent reaches into a cylinder it does not
    // hold all of — which is what separates the two refusals below.
    let path = temp_path("outside");
    std::fs::write(&path, lba_disk()).expect("writes");

    let (mut session, id) = pool(open_write(&path), raw_sector_hd());
    let medium = session.medium_mut(id).expect("pooled");
    assert_eq!(medium.geometry().determined(), Some(lba_geometry()));
    let mut sector = [0u8; 512];

    // Outside the coordinates themselves.
    for (cylinder, head, number) in [(280, 0, 1), (0, 2, 1), (0, 0, 19), (0, 0, 0)] {
        let error = medium
            .read_sector(cylinder, head, number, &mut sector)
            .expect_err("outside the geometry");
        assert_eq!(error.rule(), Some(OUTSIDE_GEOMETRY));
        assert_eq!(error.category(), ErrorCategory::NotFound);
        assert!(
            error.to_string().contains("280 cylinders of 2 heads"),
            "the refusal names the geometry it is outside: {error}"
        );
    }

    // Inside the coordinates and past the content: the last cylinder is
    // reached and not wholly held, which is a different sentence.
    let error = medium
        .read_sector(279, 1, 18, &mut sector)
        .expect_err("past the content");
    assert_eq!(error.rule(), Some(OUTSIDE_GEOMETRY));
    assert!(
        error.to_string().contains("past the content"),
        "the refusal separates the two: {error}"
    );

    // A sector is answered whole or not at all.
    let mut half = [0u8; 256];
    let error = medium
        .read_sector(0, 0, 1, &mut half)
        .expect_err("half a sector");
    assert_eq!(error.rule(), Some(PARTIAL_SECTOR));
    let error = medium
        .write_sector(0, 0, 1, &[0u8; 1024])
        .expect_err("two sectors");
    assert_eq!(error.rule(), Some(PARTIAL_SECTOR));
    assert!(
        error.to_string().contains("512 bytes"),
        "the refusal names the recording's own sector size: {error}"
    );

    drop(session);
    std::fs::remove_file(&path).ok();
}

#[test]
fn sources_that_disagree_leave_the_geometry_undetermined_and_settle_nothing() {
    // A table laid out under 2 heads of 18 sectors, carrying a volume
    // whose boot record records 4 heads of 17: two real readings of one
    // disk, and nothing here to choose between them.
    let path = temp_path("undetermined");
    std::fs::write(&path, chs_disk(&chs_volume(17, 4))).expect("writes");

    let (mut session, id) = pool(open_write(&path), raw_sector_hd());
    let medium = session.medium_mut(id).expect("pooled");
    let geometry = medium.geometry();

    assert_eq!(geometry.state(), GeometryState::Undetermined);
    assert_eq!(geometry.determined(), None);
    assert_eq!(
        geometry.conflicts().len(),
        2,
        "the head count and the sectors per track each disagree: {:?}",
        geometry.conflicts()
    );
    let conflicts = geometry.conflicts().join(" ");
    for stated in ["4", "2", "17", "18"] {
        assert!(
            conflicts.contains(stated),
            "both readings are reported: {conflicts}"
        );
    }
    assert!(
        geometry
            .readings()
            .iter()
            .any(|reading| reading.heads == Some(4))
            && geometry
                .readings()
                .iter()
                .any(|reading| reading.heads == Some(2)),
        "both readings stand"
    );

    // And the sector verbs refuse toward that state rather than picking
    // a reading to act on.
    let error = medium
        .read_sector(0, 0, 1, &mut [0u8; 512])
        .expect_err("no coordinates to address in");
    assert_eq!(error.rule(), Some(GEOMETRY_UNDETERMINED));
    assert!(error.to_string().contains("neither settles it"), "{error}");
    let error = medium
        .write_sector(0, 0, 1, &[0u8; 512])
        .expect_err("and neither does a write");
    assert_eq!(error.rule(), Some(GEOMETRY_UNDETERMINED));

    drop(session);
    std::fs::remove_file(&path).ok();
}

#[test]
fn the_loads_own_block_size_is_a_source_and_disagreeing_with_the_table_settles_nothing() {
    // A raw image records no addressable unit, so the load declares one.
    // Declaring 1,024 over a table this release reads in 512-byte blocks
    // is a contradiction about one disk — and it is reported as one,
    // never resolved by ranking the declaration above the evidence.
    let path = temp_path("declared-unit");
    std::fs::write(&path, chs_disk(&chs_volume(18, 2))).expect("writes");

    let (session, id) = pool(
        open_read(&path),
        Format::Raw {
            device: HardDrive::MbrSector.into(),
            block_bytes: 1_024,
        },
    );
    let geometry = session.medium(id).expect("pooled").geometry();
    assert_eq!(geometry.state(), GeometryState::Undetermined);
    let conflicts = geometry.conflicts().join(" ");
    assert!(
        conflicts.contains("the sector size") && conflicts.contains("1024"),
        "the declaration is one reading among the others: {conflicts}"
    );

    drop(session);
    std::fs::remove_file(&path).ok();
}

#[test]
fn a_table_that_records_no_tuple_leaves_the_boot_record_to_state_the_geometry() {
    let path = temp_path("no-tuple");
    std::fs::write(&path, lba_disk()).expect("writes");

    let (session, id) = pool(open_read(&path), raw_sector_hd());
    let geometry = session.medium(id).expect("pooled").geometry();
    assert_eq!(geometry.state(), GeometryState::Determined);
    assert_eq!(geometry.determined(), Some(lba_geometry()));
    assert!(
        !geometry
            .readings()
            .iter()
            .any(|reading| reading.source == GeometrySource::PartitionTable
                && reading.heads.is_some()),
        "an empty tuple states nothing, and nothing is inferred from it"
    );
    let extent = geometry
        .readings()
        .iter()
        .find(|reading| reading.source == GeometrySource::ExtentArithmetic)
        .expect("the extent states the cylinder count");
    assert!(
        extent.detail.contains("does not hold all of the last one"),
        "the extent says what it is short of rather than rounding it away: {}",
        extent.detail
    );

    drop(session);
    std::fs::remove_file(&path).ok();
}

#[test]
fn a_disk_whose_sources_all_stay_silent_states_no_geometry_at_all() {
    // A blank disk: no table to read tuples off, no boot record, and a
    // raw declaration that states the block size and nothing else.
    let path = temp_path("silent");
    std::fs::write(&path, vec![0u8; 1_048_576]).expect("writes");

    let (mut session, id) = pool(open_write(&path), raw_sector_hd());
    let medium = session.medium_mut(id).expect("pooled");
    let geometry = medium.geometry();
    assert_eq!(geometry.state(), GeometryState::Unstated);
    assert!(geometry.conflicts().is_empty(), "nothing disagreed");
    assert_eq!(
        geometry.readings().len(),
        1,
        "the load's declaration of the block size is all that spoke"
    );
    assert!(
        geometry.unsettled().contains(&"the head count"),
        "what is missing is named: {:?}",
        geometry.unsettled()
    );

    let error = medium
        .read_sector(0, 0, 1, &mut [0u8; 512])
        .expect_err("nothing to address in");
    assert_eq!(error.rule(), Some(GEOMETRY_UNSTATED));
    assert!(
        error.to_string().contains("the head count"),
        "the refusal names what nothing stated: {error}"
    );

    drop(session);
    std::fs::remove_file(&path).ok();
}

#[test]
fn a_block_addressed_drive_has_no_cylinder_or_head_to_be_told() {
    let path = temp_path("block-addressed");
    std::fs::write(&path, chs_disk(&chs_volume(18, 2))).expect("writes");

    let (mut session, id) = pool(
        open_write(&path),
        Format::Raw {
            device: HardDrive::MbrBlock.into(),
            block_bytes: 512,
        },
    );
    let medium = session.medium_mut(id).expect("pooled");

    // The geometry is still evidence about the recording — the artifact
    // says what it says — and the verbs are what the addressing gates.
    assert_eq!(medium.geometry().state(), GeometryState::Determined);
    let error = medium
        .read_sector(0, 0, 1, &mut [0u8; 512])
        .expect_err("block-addressed");
    assert_eq!(error.rule(), Some(NOT_SECTOR_ADDRESSED));
    assert_eq!(error.category(), ErrorCategory::Unsupported);
    assert!(
        error.to_string().contains("mbr-block-hd") && error.to_string().contains("by block"),
        "the refusal names the type and how it addresses: {error}"
    );

    drop(session);
    std::fs::remove_file(&path).ok();
}
