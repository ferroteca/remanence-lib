// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! Authored media (F60): the third fact class, exercised through the
//! public surface.
//!
//! Every walk here starts with no artifact at all. What the author states
//! at `new_media` is the whole of what the medium knows, and the tests
//! below are about that being *true* rather than merely stored: no device
//! is assumed, the coordinates answer the sector verbs, the commit point
//! is the ordinary one, and every question that would need an artifact or
//! a recording refuses by name.

use remanence::{
    Claim, DeviceSlot, FloppyDrive, Format, GeometrySource, GeometryState, HardDrive, NewMedia,
    PartitionType, Recording, RecordingGeometry, Session,
};

/// A destination no test has used, in the directory temporary files go
/// in. The rendition refuses an existing file, so each walk names its
/// own and removes it when it is done.
fn destination(tag: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("remanence-f83-{tag}-{}.img", std::process::id()));
    let _ = std::fs::remove_file(&path);
    path
}

/// The coordinates U32 authors: a 528 MB CHS disk.
fn chs() -> RecordingGeometry {
    RecordingGeometry {
        cylinders: 1024,
        heads: 16,
        sectors_per_track: 63,
        sector_bytes: 512,
    }
}

/// A small authored disk, for the walks that do not care how big it is.
fn small() -> RecordingGeometry {
    RecordingGeometry {
        cylinders: 40,
        heads: 2,
        sectors_per_track: 9,
        sector_bytes: 512,
    }
}

/// U32 — authoring a blank CHS disk and laying down its boot sector.
#[test]
fn an_authored_chs_disk_answers_in_the_coordinates_its_author_stated() {
    let mut session = Session::new();
    let disk = session
        .new_media(NewMedia::ChsDisk { geometry: chs() })
        .expect("the author states the coordinates and the medium is made whole");

    // No device is assumed. Authorship is its own fact class, and only
    // the reserved authored-to-recorded arc would bind one.
    assert_eq!(disk.device_type(), None);
    assert_eq!(disk.article(), "authored");
    assert_eq!(
        disk.authored_as(),
        Some(NewMedia::ChsDisk { geometry: chs() })
    );
    assert_eq!(
        disk.size().expect("the coordinates address it"),
        528_482_304
    );
    assert!(!disk.is_linked());

    // The geometry is the author's, and its one reading says so.
    let geometry = disk.geometry();
    assert_eq!(geometry.state(), GeometryState::Determined);
    assert_eq!(geometry.determined(), Some(chs()));
    assert!(geometry.conflicts().is_empty());
    assert!(geometry.unsettled().is_empty());
    assert_eq!(
        geometry.readings().len(),
        1,
        "there is no artifact for a second source to be read off"
    );
    assert_eq!(geometry.readings()[0].source, GeometrySource::Authorship);
    assert!(
        geometry.readings()[0].at.contains("author"),
        "the reading names who stated it: {}",
        geometry.readings()[0].at
    );

    // The authored provenance rides the medium from creation.
    let assurance = disk.assurance();
    assert_eq!(assurance.claim, Claim::Authored, "nobody opened anything");
    assert_eq!(assurance.access, remanence::AccessMode::ReadWrite);
    assert!(
        assurance
            .evidence
            .iter()
            .any(|line| line.contains("created whole by the author")),
        "{:?}",
        assurance.evidence
    );
    assert!(
        assurance
            .evidence
            .iter()
            .any(|line| line.contains("no device is assumed")),
        "{:?}",
        assurance.evidence
    );

    // A blank reads as a blank, in the author's own coordinates.
    let mut before = [0xffu8; 512];
    disk.read_sector(0, 0, 1, &mut before).expect("reads");
    assert_eq!(before, [0u8; 512], "nothing is recorded on it yet");

    let mut boot = [0u8; 512];
    boot[510] = 0x55;
    boot[511] = 0xaa;
    disk.write_sector(0, 0, 1, &boot)
        .expect("the authored geometry answers");
    assert!(
        disk.is_modified(),
        "buffered until commit, like every write"
    );

    let mut read = [0u8; 512];
    disk.read_sector(0, 0, 1, &mut read).expect("reads");
    assert_eq!(read, boot, "the session reads its own buffered truth");

    disk.commit()
        .expect("the commit point, with no artifact to journal");
    assert!(!disk.is_modified());
    let mut after = [0u8; 512];
    disk.read_sector(0, 0, 1, &mut after).expect("reads");
    assert_eq!(after, boot, "the commit made it the medium's own state");

    // The last sector the coordinates address is inside them, and the
    // one past it is not.
    let mut last = [0u8; 512];
    disk.read_sector(1023, 15, 63, &mut last)
        .expect("the last sector");
    let error = disk
        .read_sector(1024, 0, 1, &mut last)
        .expect_err("outside the authored coordinates");
    assert_eq!(
        error.rule(),
        Some(remanence::GeometryRule::OutsideGeometry.as_str())
    );
    assert!(
        error.to_string().contains("1024 cylinders of 16 heads"),
        "the refusal names the author's coordinates: {error}"
    );

    let id = disk.id();
    session
        .release_media(id)
        .expect("the one state-destroying verb");
    assert!(session.medium(id).is_none());
}

#[test]
fn a_rollback_takes_an_authored_write_back_to_what_was_committed() {
    let mut session = Session::new();
    let disk = session
        .new_media(NewMedia::ChsDisk { geometry: small() })
        .expect("created");

    disk.write_sector(0, 0, 1, &[0xa5; 512]).expect("writes");
    disk.commit().expect("commits");
    disk.write_sector(0, 0, 2, &[0x5a; 512]).expect("writes");
    assert!(disk.is_modified());
    disk.rollback().expect("discards everything buffered");
    assert!(!disk.is_modified());

    let mut kept = [0u8; 512];
    disk.read_sector(0, 0, 1, &mut kept).expect("reads");
    assert_eq!(kept, [0xa5; 512], "the committed sector survives");
    let mut gone = [0xffu8; 512];
    disk.read_sector(0, 0, 2, &mut gone).expect("reads");
    assert_eq!(gone, [0u8; 512], "and the rolled-back one never landed");
}

#[test]
fn an_authored_disk_bears_the_direct_partition_over_its_own_content() {
    let mut session = Session::new();
    let disk = session
        .new_media(NewMedia::ChsDisk { geometry: small() })
        .expect("created");

    // Nothing recorded a scheme onto it, because nothing recorded
    // anything onto it: the walk stays uniform through the direct
    // partition, which is the library's own composition and says so.
    assert_eq!(disk.partition_scheme(), None);
    let pool = disk.partitions();
    assert_eq!(pool.len(), 1);
    assert!(pool[0].is_direct());
    assert_eq!(pool[0].ordinal(), 0);
    assert!(
        pool[0]
            .provenance()
            .is_some_and(|account| account.contains("authored")),
        "a composition act is provenance: {:?}",
        pool[0].provenance()
    );
    assert!(pool[0].evidence().is_empty(), "and never evidence");
    assert!(
        !pool[0].bears_namespace(),
        "nothing declares one over a blank"
    );
    assert!(
        pool[0].check_type(PartitionType::DosPrimary).is_err(),
        "the direct partition records no type to check a reading against"
    );

    // The addressable vantage opens over the author's own content, and
    // the two doors are the same node: byte 510 of the space is the byte
    // the sector verbs wrote there.
    let mut boot = [0u8; 512];
    boot[510] = 0x55;
    boot[511] = 0xaa;
    disk.write_sector(0, 0, 1, &boot).expect("writes");

    let partition = disk
        .partition(0)
        .expect("an authored blank bears its direct partition");
    assert!(partition.partition().is_addressable());
    let mut signature = [0u8; 2];
    partition
        .volume()
        .expect("the addressable vantage opens over authored content")
        .read_at(510, &mut signature)
        .expect("reads within the extent");
    assert_eq!(signature, [0x55, 0xaa]);

    // The namespace vantage does not: nothing recorded one onto these
    // coordinates, so there is no boot record for the FAT seam to read.
    let partition = disk.partition(0).expect("the direct partition");
    assert!(partition.filesystem().is_none());
    let error = disk
        .partition(0)
        .expect("the direct partition")
        .filesystem_as("fat")
        .expect_err("an authored blank bears no namespace");
    assert_eq!(
        error.category(),
        remanence::ErrorCategory::InvalidImage,
        "the FAT adapter read the content and found no volume: {error}"
    );

    // And the arc that records one refuses here too, for its own
    // reason: a layout is recorded onto a manufactured article, and
    // this medium's facts are coordinates its author stated.
    let error = disk
        .partition(0)
        .expect("the direct partition")
        .record_as(Recording::Dos144)
        .expect_err("a CHS disk is not a blank article");
    assert!(
        error.to_string().contains("the author's own coordinates"),
        "the refusal names why: {error}"
    );
}

/// U35 — the authored-to-recorded arc: a blank DOS floppy, formatted,
/// with files on it, read back through the ordinary seams.
#[test]
fn a_layout_recorded_onto_a_blank_article_makes_it_a_dos_floppy() {
    for (kind, layout, drive, bytes) in [
        (
            NewMedia::Flexible525Hd,
            Recording::Dos12,
            FloppyDrive::Pc525Hd,
            1_228_800u64,
        ),
        (
            NewMedia::Flexible35Hd,
            Recording::Dos144,
            FloppyDrive::Pc35Hd,
            1_474_560,
        ),
    ] {
        let mut session = Session::new();
        let id = session.new_media(kind).expect("created").id();

        // Before the arc it is the article and nothing else.
        let disk = session.medium_mut(id).expect("pooled");
        assert_eq!(disk.device_type(), None);
        assert_eq!(disk.recorded_as(), None);
        assert_eq!(disk.geometry().state(), GeometryState::Unstated);
        assert!(disk.size().is_err(), "nothing is recorded on it yet");

        disk.partition(0)
            .expect("a blank article bears its direct partition")
            .record_as(layout)
            .expect("the layout records onto the article it fits");

        // Afterwards it is a recording, and every question says so.
        let disk = session.medium(id).expect("pooled");
        assert_eq!(disk.recorded_as(), Some(layout));
        assert_eq!(disk.device_type(), Some(drive.into()));
        assert_eq!(disk.size().expect("it has content now"), bytes);
        assert_eq!(disk.article(), kind.article(), "the article is unchanged");

        let geometry = disk.geometry();
        assert_eq!(geometry.state(), GeometryState::Determined);
        assert_eq!(geometry.determined(), Some(layout.geometry()));
        assert_eq!(geometry.readings().len(), 1, "the layout is the one source");
        assert_eq!(geometry.readings()[0].source, GeometrySource::Recording);

        // The sector verbs address in the layout's coordinates, and the
        // first sector is the boot record it just wrote.
        let disk = session.medium_mut(id).expect("pooled");
        let mut boot = [0u8; 512];
        disk.read_sector(0, 0, 1, &mut boot).expect("reads");
        assert_eq!(&boot[510..], &[0x55, 0xaa], "the boot signature");
        assert_eq!(boot[21], layout.media_descriptor(), "the media descriptor");

        // The namespace opens by evidence — nothing is declared — and
        // the file verbs are the delivered ones.
        let mut files = disk
            .partition(0)
            .expect("the direct partition")
            .filesystem()
            .expect("the boot record just recorded determines FAT");
        assert_eq!(files.kind().expect("a recognized volume"), "FAT12");
        assert!(files.entries("").expect("lists").is_empty(), "a fresh disk");

        files
            .write_file("AUTOEXEC.BAT", b"@ECHO OFF\r\nPATH C:\\DOS\r\n")
            .expect("writes");
        files.make_directory("DATA").expect("makes a directory");
        files
            .write_file("DATA/NOTES.TXT", b"recorded, not found\r\n")
            .expect("writes into it");

        // Buffered until the commit point, like every other write (P2).
        drop(files);
        let disk = session.medium_mut(id).expect("pooled");
        assert!(disk.is_modified());
        disk.commit().expect("commits with no artifact beneath it");
        assert!(!disk.is_modified());

        let mut files = disk
            .partition(0)
            .expect("the direct partition")
            .filesystem()
            .expect("still FAT");
        let listed: Vec<String> = files
            .entries("")
            .expect("lists")
            .into_iter()
            .map(|entry| entry.name)
            .collect();
        assert_eq!(listed, vec!["AUTOEXEC.BAT".to_owned(), "DATA".to_owned()]);
        assert_eq!(
            files.read_file("DATA/NOTES.TXT").expect("reads back"),
            b"recorded, not found\r\n"
        );

        // And a drive takes it now, which is the whole point of binding
        // the device the layout is recorded for.
        drop(files);
        let mut bay = session.add_device(drive).expect("added");
        bay.insert(id)
            .expect("the drive the layout is recorded for");
    }
}

/// U35, to the end: the disk is made, formatted, filled, and written
/// out as the raw image an emulator reads — which then loads back as
/// the disk it was.
#[test]
fn a_recorded_floppy_writes_out_as_a_raw_image_and_loads_back() {
    let path = destination("roundtrip");
    let mut session = Session::new();
    let id = session
        .new_media(NewMedia::Flexible35Hd)
        .expect("created")
        .id();
    let disk = session.medium_mut(id).expect("pooled");
    disk.partition(0)
        .expect("the direct partition")
        .record_as(Recording::Dos144)
        .expect("records");
    let mut files = disk
        .partition(0)
        .expect("the direct partition")
        .filesystem()
        .expect("FAT12");
    files
        .write_file("README.TXT", b"made, not found\r\n")
        .expect("writes");
    drop(files);

    // The plan states everything before a byte moves, and writing adds
    // nothing to the account.
    let disk = session.medium_mut(id).expect("pooled");
    let planned = disk.describe_raw().expect("plans");
    assert_eq!(planned.path, None, "a plan writes nothing");
    assert_eq!(planned.artifact_bytes, 1_474_560);
    assert_eq!(planned.sectors_written, 2_880);
    assert_eq!(planned.geometry, Recording::Dos144.geometry());
    assert!(
        planned.uncommitted_extents > 0,
        "the file written above is not committed yet"
    );

    // What a raw artifact cannot carry is named, not dropped quietly.
    let codes: Vec<&str> = planned
        .declared_loss
        .iter()
        .map(|loss| loss.code.as_str())
        .collect();
    for named in [
        "article",
        "device-type",
        "authored-provenance",
        "recorded-layout",
    ] {
        assert!(
            codes.contains(&named),
            "{named} is not in the account: {codes:?}"
        );
    }

    // The rendition is of committed state, so the commit comes first.
    disk.commit().expect("commits");
    let written = disk.write_raw(&path).expect("writes the image");
    assert_eq!(
        written.path.as_deref(),
        Some(path.display().to_string().as_str())
    );
    assert_eq!(written.artifact_bytes, 1_474_560);
    assert_eq!(
        written.uncommitted_extents, 0,
        "nothing was left behind once it was committed"
    );
    assert_eq!(
        written.declared_loss, planned.declared_loss,
        "the write adds nothing to the account the plan stated"
    );
    assert_eq!(
        std::fs::metadata(&path).expect("the file is there").len(),
        1_474_560,
        "the artifact is the 1.44 MB disk"
    );

    // An existing destination is a refusal, never an overwrite.
    let error = session
        .medium_mut(id)
        .expect("pooled")
        .write_raw(&path)
        .expect_err("something is already there");
    assert!(
        error.to_string().contains("already there"),
        "the refusal names why: {error}"
    );

    // And the test the whole journey rests on: the artifact loads back
    // as the disk that was recorded, read by evidence this time.
    let mut reader = Session::new();
    let loaded = reader
        .load_media(
            std::fs::File::open(&path).expect("opens"),
            Format::Raw {
                device: FloppyDrive::Pc35Hd.into(),
                block_bytes: 512,
            },
        )
        .expect("a raw reading of a floppy");
    assert_eq!(
        loaded.geometry().determined(),
        Some(Recording::Dos144.geometry()),
        "the BPB states the coordinates the layout recorded"
    );
    let boot = loaded
        .geometry()
        .readings()
        .iter()
        .find(|reading| reading.source == GeometrySource::BootRecord)
        .expect("the boot record is a source now, where the layout was before");
    assert_eq!(boot.sectors_per_track, Some(18));
    // The namespace is *declared* here, where the recorded medium's was
    // determined — and that difference is the rendition's account made
    // flesh. On the medium, the author's own `record_as` said it was
    // FAT12; the artifact carries the bytes that declaration wrote and
    // not the declaration, so a reader of it says so itself.
    assert!(
        loaded
            .partition(0)
            .expect("the direct partition")
            .filesystem()
            .is_none(),
        "nothing in a raw artifact declares what is on it"
    );
    let mut files = loaded
        .partition(0)
        .expect("the direct partition")
        .filesystem_as("fat")
        .expect("the boot record bears the reading");
    assert_eq!(
        files.read_file("README.TXT").expect("reads"),
        b"made, not found\r\n",
        "the file written before the rendition is in the artifact"
    );

    drop(files);
    drop(reader);
    let _ = std::fs::remove_file(&path);
}

/// U35 with a label: the LABEL-command analog on the namespace vantage,
/// set on the authored disk and read back off the raw artifact.
#[test]
fn a_label_set_on_the_recorded_disk_travels_into_the_raw_image() {
    let path = destination("labelled");
    let mut session = Session::new();
    let id = session
        .new_media(NewMedia::Flexible35Hd)
        .expect("created")
        .id();
    let disk = session.medium_mut(id).expect("pooled");
    disk.partition(0)
        .expect("the direct partition")
        .record_as(Recording::Dos144)
        .expect("records");

    let mut files = disk
        .partition(0)
        .expect("the direct partition")
        .filesystem()
        .expect("FAT12");
    files.set_label(Some("my disk")).expect("labels");
    // The same handle answers the volume as it now stands, without a
    // recomposition in between.
    let label = files.label().expect("answers").expect("FAT has labels");
    assert_eq!(label.name.as_deref(), Some("MY DISK"), "uppercased");
    assert_eq!(label.answered_by.as_deref(), Some("root-directory-entry"));
    let boot = label
        .readings
        .iter()
        .find(|reading| reading.source == "boot-record-field")
        .expect("the boot record was consulted");
    assert_eq!(
        boot.stored.as_deref(),
        Some("NO NAME"),
        "the boot record's field keeps what the recording laid down"
    );
    // A label outside the grammar is refused naming its rule, through
    // the same door.
    let error = files
        .set_label(Some("TWELVE CHARS"))
        .expect_err("twelve characters");
    assert_eq!(error.rule(), Some("label-too-long"));
    files
        .write_file("README.TXT", b"labelled\r\n")
        .expect("writes");
    drop(files);

    let disk = session.medium_mut(id).expect("pooled");
    disk.commit().expect("commits");
    disk.write_raw(&path).expect("writes the image");

    // The artifact carries the entry: a reader by evidence answers the
    // label the author set, and may relabel through the same door.
    let mut reader = Session::new();
    let loaded = reader
        .load_media(
            std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&path)
                .expect("opens writable"),
            Format::Raw {
                device: FloppyDrive::Pc35Hd.into(),
                block_bytes: 512,
            },
        )
        .expect("a raw reading of a floppy");
    let mut files = loaded
        .partition(0)
        .expect("the direct partition")
        .filesystem_as("fat")
        .expect("the boot record bears the reading");
    let label = files.label().expect("answers").expect("FAT has labels");
    assert_eq!(label.name.as_deref(), Some("MY DISK"));
    files
        .set_label(None)
        .expect("a loaded disk relabels through the same door DOS's LABEL used");
    let label = files.label().expect("answers").expect("FAT has labels");
    assert_eq!(label.name, None, "unlabeled, the entry removed");

    drop(files);
    drop(reader);
    let _ = std::fs::remove_file(&path);
}

/// The rendition refuses what it cannot render, each by its own rule.
#[test]
fn a_medium_with_no_sectors_to_write_refuses_by_name() {
    let mut session = Session::new();

    // A blank article has no content at all.
    let blank = session
        .new_media(NewMedia::Flexible35Hd)
        .expect("created")
        .id();
    let error = session
        .medium_mut(blank)
        .expect("pooled")
        .describe_raw()
        .expect_err("a blank article renders nothing");
    assert!(
        error.to_string().contains("nothing recorded on it"),
        "the refusal names what it is: {error}"
    );

    // An archive is reached by name and has no sectors at all.
    let mut listing = Session::new();
    assert!(
        listing
            .new_media(NewMedia::ChsDisk { geometry: small() })
            .expect("created")
            .describe_raw()
            .is_ok(),
        "an authored disk states its own coordinates and renders in them"
    );
}

/// The arc's own refusals: it records onto a blank article, once, and
/// only where the layout fits the article.
#[test]
fn the_arc_records_onto_a_blank_article_once_and_only_where_it_fits() {
    let mut session = Session::new();

    // A layout onto the wrong article: the check is the catalog's.
    let wrong = session
        .new_media(NewMedia::Flexible525Hd)
        .expect("created")
        .id();
    let error = session
        .medium_mut(wrong)
        .expect("pooled")
        .partition(0)
        .expect("the direct partition")
        .record_as(Recording::Dos144)
        .expect_err("the 1.44 MB layout does not fit a 5.25-inch disk");
    let message = error.to_string();
    assert!(
        message.contains("flexible-3.5-hd") && message.contains("flexible-5.25-hd"),
        "the refusal names both articles: {message}"
    );

    // A layout onto an article nothing records onto at all.
    let soft = session
        .new_media(NewMedia::Flexible525Soft)
        .expect("created")
        .id();
    assert!(
        session
            .medium_mut(soft)
            .expect("pooled")
            .partition(0)
            .expect("the direct partition")
            .record_as(Recording::Dos12)
            .is_err(),
        "the high-density layout is not laid onto a double-density disk"
    );

    // And twice onto the same blank.
    let twice = session
        .new_media(NewMedia::Flexible35Hd)
        .expect("created")
        .id();
    session
        .medium_mut(twice)
        .expect("pooled")
        .partition(0)
        .expect("the direct partition")
        .record_as(Recording::Dos144)
        .expect("records");
    let error = session
        .medium_mut(twice)
        .expect("pooled")
        .partition(0)
        .expect("the direct partition")
        .record_as(Recording::Dos144)
        .expect_err("the arc records once");
    assert!(
        error.to_string().contains("already carries"),
        "the refusal names what is already there: {error}"
    );
}

/// Every claimed layout is a published disk, spelled the same way on
/// every surface.
#[test]
fn the_recorded_layouts_are_an_enumerated_claim() {
    let claimed: Vec<&str> = Recording::claimed()
        .iter()
        .map(|claim| claim.id())
        .collect();
    assert_eq!(claimed, vec!["dos-1.2", "dos-1.44"]);
    for claim in Recording::claimed() {
        let layout = Recording::declared(claim.id()).expect("claimed");
        assert_eq!(layout.article(), claim.article());
        assert_eq!(layout.geometry(), claim.geometry());
        assert_eq!(layout.name(), claim.name());
    }
    let error = Recording::declared("dos-360k").expect_err("refused");
    assert!(
        error.to_string().contains("dos-1.2"),
        "an unclaimed layout is refused naming what is claimed: {error}"
    );
}

#[test]
fn a_blank_article_is_the_article_and_states_nothing_else() {
    let mut session = Session::new();
    for (kind, article) in [
        (NewMedia::Flexible525Soft, "flexible-5.25-soft"),
        (NewMedia::Flexible525HardTen, "flexible-5.25-hard-10"),
        (NewMedia::Flexible525Hd, "flexible-5.25-hd"),
        (NewMedia::Flexible35Hd, "flexible-3.5-hd"),
    ] {
        let blank = session.new_media(kind).expect("created");
        assert_eq!(blank.article(), article);
        assert_eq!(blank.device_type(), None);
        assert_eq!(blank.authored_as(), Some(kind));
        assert_eq!(
            blank.geometry().state(),
            GeometryState::Unstated,
            "nothing is recorded on it, so it states no coordinates"
        );
        assert!(blank.geometry().readings().is_empty());
        assert!(!blank.is_modified());

        // It has an article and no content, and every content verb says
        // exactly that.
        assert!(blank.size().is_err());
        let error = blank
            .write_sector(0, 0, 1, &[0u8; 512])
            .expect_err("nothing is recorded on it");
        assert_eq!(
            error.rule(),
            Some(remanence::GeometryRule::NotSectorAddressed.as_str())
        );
        assert!(
            error.to_string().contains("blank article"),
            "names what it is: {error}"
        );

        // The walk still passes through a direct partition, which has no
        // extent to be a position within.
        assert_eq!(blank.partitions().len(), 1);
        assert!(blank.partitions()[0].is_direct());
        assert!(!blank.partitions()[0].is_addressable());
        assert_eq!(blank.partitions()[0].start_bytes(), None);
    }
}

#[test]
fn an_authored_blank_goes_in_no_drive_at_all() {
    let mut session = Session::new();
    let authored = session
        .new_media(NewMedia::ChsDisk { geometry: small() })
        .expect("created")
        .id();

    // A CHS disk the author made could be seated in a sector-addressed
    // hard drive on the article alone — and is not, because nothing
    // recorded it and the edge weighs the recording (D19).
    let mut drive = session.add_device(HardDrive::MbrSector).expect("added");
    let error = drive
        .insert(authored)
        .expect_err("no drive takes an authored blank");
    let message = error.to_string();
    assert!(
        message.contains("assumes no device"),
        "names why: {message}"
    );
    assert!(
        message.contains("mbr-sector-hd"),
        "and names what the slot takes: {message}"
    );
    assert!(!drive.is_occupied());

    // Not the archive receiver either: an authored blank is no archive.
    let mut receiver = session.add_device(DeviceSlot::Archive).expect("added");
    assert!(receiver.insert(authored).is_err());
}

#[test]
fn an_authored_medium_has_no_artifact_and_says_so() {
    let mut session = Session::new();
    let disk = session
        .new_media(NewMedia::ChsDisk { geometry: small() })
        .expect("created");

    assert_eq!(disk.path(), None, "there is no artifact to name");
    assert_eq!(disk.image_path(), None);
    assert_eq!(disk.image_size_bytes(), 0);

    let mut buf = [0u8; 16];
    let error = disk.read_at(0, &mut buf).expect_err("no artifact plane");
    assert!(
        error.to_string().contains("created whole by the author"),
        "{error}"
    );
    assert!(disk.format().is_err(), "no image format recognized it");
    assert!(disk.inspect().is_err(), "and there is no layout to report");
    assert!(
        disk.bitstream().is_err(),
        "no device recorded it, so no flux path answers"
    );

    // The identification is the article and nothing beneath it.
    let identification = disk.identify();
    assert_eq!(identification.layers.len(), 1);
    assert_eq!(identification.layers[0].id, "authored");
    assert!(
        identification
            .evidence
            .iter()
            .any(|line| line.contains("third fact class")),
        "{:?}",
        identification.evidence
    );
}

#[test]
fn what_the_author_states_is_checked_when_they_state_it() {
    let mut session = Session::new();
    for geometry in [
        RecordingGeometry {
            cylinders: 0,
            ..small()
        },
        RecordingGeometry {
            heads: 0,
            ..small()
        },
        RecordingGeometry {
            sectors_per_track: 0,
            ..small()
        },
        RecordingGeometry {
            sector_bytes: 0,
            ..small()
        },
    ] {
        let error = session
            .new_media(NewMedia::ChsDisk { geometry })
            .expect_err("a geometry is whole or it is nothing");
        assert!(
            error.to_string().contains("whole or it is nothing"),
            "{error}"
        );
    }
    assert!(
        session.media().is_empty(),
        "a refused creation pools nothing"
    );
}

#[test]
fn the_authored_catalog_is_an_enumerated_claim() {
    // P3, at the boundary where a kind arrives as text.
    for claim in NewMedia::claimed() {
        let geometry = claim.takes_geometry().then(small);
        let kind = NewMedia::declared(claim.id(), geometry).expect("claimed");
        assert_eq!(kind.id(), claim.id());
        assert_eq!(kind.article(), claim.article());
    }
    let error = NewMedia::declared("floppy", None).expect_err("a classification checks nothing");
    let message = error.to_string();
    assert!(
        message.contains("floppy"),
        "names what was asked: {message}"
    );
    assert!(
        message.contains("chs-disk"),
        "names what is authored: {message}"
    );
}

#[test]
fn media_authored_and_media_loaded_are_the_same_pool() {
    // The fact classes differ; the pool does not. A session holds both,
    // and releases either, with the same three verbs.
    let mut session = Session::new();
    let first = session
        .new_media(NewMedia::ChsDisk { geometry: small() })
        .expect("created")
        .id();
    let second = session
        .new_media(NewMedia::Flexible525Soft)
        .expect("created")
        .id();
    assert_eq!(session.media(), vec![first, second]);
    assert_ne!(first, second, "an identity is never reused");

    session.release_media(first).expect("released");
    assert!(session.medium(first).is_none());
    assert!(session.medium(second).is_some());
    assert_eq!(session.media(), vec![second]);
}
