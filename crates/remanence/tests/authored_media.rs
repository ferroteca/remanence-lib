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
    Claim, DeviceSlot, GeometrySource, GeometryState, HardDrive, NewMedia, PartitionType,
    RecordingGeometry, Session,
};

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
    disk.get_sector(0, 0, 1, &mut before).expect("reads");
    assert_eq!(before, [0u8; 512], "nothing is recorded on it yet");

    let mut boot = [0u8; 512];
    boot[510] = 0x55;
    boot[511] = 0xaa;
    disk.put_sector(0, 0, 1, &boot)
        .expect("the authored geometry answers");
    assert!(
        disk.is_modified(),
        "buffered until commit, like every write"
    );

    let mut read = [0u8; 512];
    disk.get_sector(0, 0, 1, &mut read).expect("reads");
    assert_eq!(read, boot, "the session reads its own buffered truth");

    disk.commit()
        .expect("the commit point, with no artifact to journal");
    assert!(!disk.is_modified());
    let mut after = [0u8; 512];
    disk.get_sector(0, 0, 1, &mut after).expect("reads");
    assert_eq!(after, boot, "the commit made it the medium's own state");

    // The last sector the coordinates address is inside them, and the
    // one past it is not.
    let mut last = [0u8; 512];
    disk.get_sector(1023, 15, 63, &mut last)
        .expect("the last sector");
    let error = disk
        .get_sector(1024, 0, 1, &mut last)
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

    disk.put_sector(0, 0, 1, &[0xa5; 512]).expect("writes");
    disk.commit().expect("commits");
    disk.put_sector(0, 0, 2, &[0x5a; 512]).expect("writes");
    assert!(disk.is_modified());
    disk.rollback().expect("discards everything buffered");
    assert!(!disk.is_modified());

    let mut kept = [0u8; 512];
    disk.get_sector(0, 0, 1, &mut kept).expect("reads");
    assert_eq!(kept, [0xa5; 512], "the committed sector survives");
    let mut gone = [0xffu8; 512];
    disk.get_sector(0, 0, 2, &mut gone).expect("reads");
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
        pool[0].as_type(PartitionType::DosPrimary).is_err(),
        "the direct partition records no type to check a reading against"
    );

    // The addressable vantage opens over the author's own content, and
    // the two doors are the same node: byte 510 of the space is the byte
    // the sector verbs wrote there.
    let mut boot = [0u8; 512];
    boot[510] = 0x55;
    boot[511] = 0xaa;
    disk.put_sector(0, 0, 1, &boot).expect("writes");

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

    // The namespace vantage does not: nothing recorded one, and the arc
    // that would record one is reserved.
    let partition = disk.partition(0).expect("the direct partition");
    assert!(partition.filesystem().is_none());
    let error = disk
        .partition(0)
        .expect("the direct partition")
        .filesystem_as("fat")
        .expect_err("an authored blank bears no namespace");
    assert!(
        error.to_string().contains("authored-to-recorded arc"),
        "the refusal names what is reserved: {error}"
    );
}

#[test]
fn a_blank_article_is_the_article_and_states_nothing_else() {
    let mut session = Session::new();
    for (kind, article) in [
        (NewMedia::Flexible525Soft, "flexible-5.25-soft"),
        (NewMedia::Flexible525HardTen, "flexible-5.25-hard-10"),
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
            .put_sector(0, 0, 1, &[0u8; 512])
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
