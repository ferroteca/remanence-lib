// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! Degraded, evidence-bounded access (P28) over a truncated raw floppy the
//! project builds itself: the explicit 1.44 MiB FAT12 declaration a
//! shorter source cannot satisfy.
//!
//! The journey these tests hold to is the whole point of the principle. A
//! deficient image is neither accepted whole nor thrown away whole: the
//! caller is told before it reads a byte, the data that is wholly present
//! reads, the data that is not is refused by name with its range, and
//! write authority is gone for the session's life.

use std::path::{Path, PathBuf};

use remanence::{
    AccessMode, AssuranceCondition, AssuranceOutcome, ByteRange, ErrorCategory, Format, HardDrive,
    MediaId, Medium, PartitionRule, Session,
};

mod common;
use common::{open_read, open_write};

/// The space one partition of `medium` composes, reached through the
/// door that opens on it. One node, both vantages (D26), and the file
/// verbs live on it and nowhere else (P19).
fn fs(medium: &mut Medium, ordinal: u32) -> remanence::StorageSpace<'_> {
    let partition = medium
        .partition(ordinal)
        .expect("the pool bears this partition");
    if partition.partition().bears_namespace() {
        partition
            .filesystem()
            .expect("the declared type determines one")
    } else {
        partition
            .filesystem_as("fat")
            .expect("these images are FAT")
    }
}

/// The 1.44 MiB floppy's declared size: 2880 sectors of 512 bytes.
const DECLARED: u64 = 1_474_560;

/// What the truncated source actually holds — past the boot record, both
/// FAT copies and the root directory, and into the data area.
const OBSERVED: u64 = 1_000_000;

/// A file whose clusters land wholly inside the truncated source.
const NEAR: &str = "NEAR.BIN";

/// A file whose cluster chain runs off the end of it.
const FAR: &str = "FAR.BIN";

fn temp_path(tag: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nonce = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "remanence-degraded-{tag}-{}-{nonce}.img",
        std::process::id()
    ))
}

/// A 1.44 MiB-floppy-shaped FAT12 volume: 2880 sectors, 18 sectors/track,
/// 2 heads, 224 root entries, two 9-sector FAT copies.
fn floppy_1440() -> Vec<u8> {
    const TOTAL_SECTORS: usize = 2880;
    let mut image = vec![0u8; TOTAL_SECTORS * 512];

    image[0] = 0xeb;
    image[1] = 0x3c;
    image[2] = 0x90;
    image[3..11].copy_from_slice(b"REMANENC");
    image[11..13].copy_from_slice(&512u16.to_le_bytes()); // bytes/sector
    image[13] = 1; // sectors/cluster
    image[14..16].copy_from_slice(&1u16.to_le_bytes()); // reserved
    image[16] = 2; // FATs
    image[17..19].copy_from_slice(&224u16.to_le_bytes()); // root entries
    image[19..21].copy_from_slice(&(TOTAL_SECTORS as u16).to_le_bytes());
    image[21] = 0xf0; // media descriptor
    image[22..24].copy_from_slice(&9u16.to_le_bytes()); // sectors/FAT
    image[24..26].copy_from_slice(&18u16.to_le_bytes()); // sectors/track
    image[26..28].copy_from_slice(&2u16.to_le_bytes()); // heads
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

/// Wraps a volume in a one-partition MBR disk, so the leading sector is a
/// partition table rather than a boot record.
fn mbr_disk(volume: &[u8]) -> Vec<u8> {
    let start = 2048usize;
    let mut disk = vec![0u8; start * 512 + volume.len()];
    disk[446 + 4] = 0x01; // FAT12
    disk[446 + 8..446 + 12].copy_from_slice(&(start as u32).to_le_bytes());
    disk[446 + 12..446 + 16].copy_from_slice(&((volume.len() / 512) as u32).to_le_bytes());
    disk[510] = 0x55;
    disk[511] = 0xaa;
    disk[start * 512..].copy_from_slice(volume);
    disk
}

fn near_content() -> Vec<u8> {
    (0..4_096u32).map(|n| (n % 251) as u8).collect()
}

/// Big enough that its chain leaves the truncated source: the data area
/// starts at byte 16,896, so 1.2 MB of clusters ends past 1,000,000.
fn far_content() -> Vec<u8> {
    (0..1_200_000u32).map(|n| (n % 241) as u8).collect()
}

/// Writes a whole floppy holding both files, committed and closed — the
/// verified state a truncation later takes bytes off the end of.
fn build_floppy(path: &Path) {
    std::fs::write(path, floppy_1440()).expect("image writes");
    let mut session = Session::new();
    let medium = session
        .load_media(
            open_write(path),
            Format::Raw {
                device: HardDrive::MbrSector.into(),
                block_bytes: 512,
            },
        )
        .expect("the whole image loads");
    let partition = only_partition_of(medium);
    fs(medium, partition)
        .write_file(NEAR, &near_content())
        .expect("writes the near file");
    fs(medium, partition)
        .write_file(FAR, &far_content())
        .expect("writes the far file");
    medium.commit().expect("commits");
}

/// The ordinal these images' content is reached through. A bare floppy
/// records no partition scheme, so its pool bears exactly one member —
/// the direct partition at ordinal 0, which is the library's own
/// composition of the whole content and says so in its provenance rather
/// than offering it as something the medium declared (P16, P17).
fn only_partition_of(medium: &Medium) -> u32 {
    let partitions = medium.partitions();
    assert_eq!(
        partitions.len(),
        1,
        "a bare floppy records no scheme, so its pool bears one partition"
    );
    assert!(
        partitions[0].is_direct(),
        "and that one is the library's own composition, not a table entry"
    );
    assert!(
        partitions[0].provenance().is_some() && partitions[0].evidence().is_empty(),
        "a composition act is provenance, never evidence (P4)"
    );
    assert!(
        partitions[0].volume_id().is_some(),
        "which still composes the one volume the report issues for it"
    );
    partitions[0].ordinal()
}

fn truncate(path: &Path, len: u64) {
    std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .expect("opens for truncation")
        .set_len(len)
        .expect("truncates");
}

/// Builds the whole floppy, truncates it, and pools what is left under
/// a handle affording what `afford` says.
fn truncated(tag: &str, afford: Afford) -> (PathBuf, Session, MediaId) {
    let path = temp_path(tag);
    build_floppy(&path);
    truncate(&path, OBSERVED);
    let source = match afford {
        Afford::Read => open_read(&path),
        Afford::Write => open_write(&path),
    };
    let mut session = Session::new();
    let id = session
        .load_media(
            source,
            Format::Raw {
                device: HardDrive::MbrSector.into(),
                block_bytes: 512,
            },
        )
        .expect("a truncated source still loads, degraded")
        .id();
    (path, session, id)
}

/// What the caller's own open affords, in the shape these tests declare
/// it: the amended P7 asks the handle one question, so the test says
/// which answer it wants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Afford {
    Read,
    Write,
}

#[test]
fn a_whole_source_is_verified_and_keeps_its_write_authority() {
    let path = temp_path("verified");
    build_floppy(&path);

    let mut session = Session::new();
    let medium = session
        .load_media(
            open_write(&path),
            Format::Raw {
                device: HardDrive::MbrSector.into(),
                block_bytes: 512,
            },
        )
        .expect("the whole image loads");
    let assurance = medium.assurance().clone();

    assert_eq!(assurance.outcome, AssuranceOutcome::Verified);
    assert_eq!(assurance.condition, None);
    assert_eq!(assurance.readable, vec![ByteRange::new(0, DECLARED)]);
    assert_eq!(assurance.access, AccessMode::ReadWrite);
    assert_eq!(assurance.first_unavailable_byte, None);
    assert_eq!(medium.mode(), AccessMode::ReadWrite);

    let partition = only_partition_of(medium);
    assert_eq!(
        fs(medium, partition).read_file(FAR).expect("reads whole"),
        far_content()
    );
    drop(session);
    std::fs::remove_file(&path).ok();
}

#[test]
fn a_truncated_floppy_declares_its_shortfall_before_anything_is_read() {
    let (path, mut session, attachment) = truncated("notifies", Afford::Write);
    let assurance = session
        .medium_mut(attachment)
        .expect("medium")
        .assurance()
        .clone();

    assert_eq!(assurance.outcome, AssuranceOutcome::Degraded);
    assert_eq!(
        assurance.condition,
        Some(AssuranceCondition::SourceTruncated)
    );
    assert_eq!(assurance.declared_bytes, Some(DECLARED));
    assert_eq!(assurance.observed_bytes, Some(OBSERVED));
    assert_eq!(assurance.first_unavailable_byte, Some(OBSERVED));
    assert_eq!(assurance.readable, vec![ByteRange::new(0, OBSERVED)]);
    assert_eq!(assurance.readable[0].len(), OBSERVED);

    let evidence = assurance.evidence.join("\n");
    assert!(
        evidence.contains("2880 sectors") && evidence.contains(&DECLARED.to_string()),
        "the declaration is stated: {evidence}"
    );
    assert!(
        evidence.contains(&OBSERVED.to_string()),
        "the observation is stated: {evidence}"
    );
    assert!(
        evidence.contains("write authority is withheld"),
        "the withheld authority is stated: {evidence}"
    );

    drop(session);
    std::fs::remove_file(&path).ok();
}

#[test]
fn a_write_intent_open_reports_its_effective_read_only_mode() {
    let (path, mut session, attachment) = truncated("effective-mode", Afford::Write);
    let medium = session.medium_mut(attachment).expect("medium");

    assert_eq!(
        medium.mode(),
        AccessMode::ReadOnly,
        "the evidence, not the declaration, settles the effective mode"
    );
    assert_eq!(medium.assurance().access, AccessMode::ReadOnly);

    drop(session);
    std::fs::remove_file(&path).ok();
}

#[test]
fn every_mutation_path_including_commit_returns_the_condition() {
    let (path, mut session, attachment) = truncated("mutations", Afford::Write);
    let medium = session.medium_mut(attachment).expect("medium");
    let partition = only_partition_of(medium);

    // Each space is taken in turn rather than four at once: a namespace
    // holds its borrow of what it reads through until it is dropped, so
    // the medium is free for the next one only after the last is done
    // with it.
    let mut refusals: Vec<remanence::Error> = Vec::new();
    refusals.push(
        fs(medium, partition)
            .write_file("NEW.BIN", b"nope")
            .expect_err("write_file is denied"),
    );
    refusals.push(
        fs(medium, partition)
            .get_file(NEAR)
            .expect("the entry is inside the readable extent")
            .write_at(0, b"nope")
            .expect_err("the ranged write is denied"),
    );
    refusals.push(
        fs(medium, partition)
            .resize_file(NEAR, 10)
            .expect_err("resize_file is denied"),
    );
    refusals.push(
        fs(medium, partition)
            .make_directory("SUB")
            .expect_err("make_directory is denied"),
    );
    refusals.push(medium.commit().expect_err("commit is denied"));

    for refusal in &refusals {
        assert_eq!(
            refusal.category(),
            ErrorCategory::ReadOnly,
            "how to behave: {refusal}"
        );
        assert_eq!(
            refusal.rule(),
            Some(AssuranceCondition::SourceTruncated.as_str()),
            "which condition withheld it: {refusal}"
        );
        assert_eq!(
            refusal.rule().and_then(AssuranceCondition::from_identity),
            Some(AssuranceCondition::SourceTruncated),
            "the identity reads back into this seam's set"
        );
    }

    drop(session);
    std::fs::remove_file(&path).ok();
}

#[test]
fn wholly_present_directory_and_file_data_still_read() {
    let (path, mut session, attachment) = truncated("reads", Afford::Read);
    let medium = session.medium_mut(attachment).expect("medium");
    let partition = only_partition_of(medium);

    let names: Vec<String> = fs(medium, partition)
        .entries("")
        .expect("the root directory is wholly present, so it lists")
        .into_iter()
        .map(|entry| entry.name)
        .collect();
    assert!(
        names.iter().any(|name| name == NEAR) && names.iter().any(|name| name == FAR),
        "a directory lists both entries even where one file's data is gone: {names:?}"
    );

    assert_eq!(
        fs(medium, partition)
            .stat(FAR)
            .expect("stat reads the record")
            .map(|entry| entry.size_bytes),
        Some(far_content().len() as u64),
        "the record is present, so the entry is answered"
    );

    assert_eq!(
        fs(medium, partition)
            .read_file(NEAR)
            .expect("the near file reads"),
        near_content()
    );

    drop(session);
    std::fs::remove_file(&path).ok();
}

#[test]
fn a_crossing_file_is_refused_whole_rather_than_clipped() {
    let (path, mut session, attachment) = truncated("crossing", Afford::Read);
    let medium = session.medium_mut(attachment).expect("medium");
    let partition = only_partition_of(medium);

    let refusal = fs(medium, partition)
        .read_file(FAR)
        .expect_err("a chain leaving the readable extent is refused");
    assert_eq!(refusal.category(), ErrorCategory::Unavailable);
    assert_eq!(
        refusal.rule(),
        Some(AssuranceCondition::SourceTruncated.as_str())
    );
    let message = refusal.to_string();
    assert!(
        message.contains(&OBSERVED.to_string()) && message.contains(&DECLARED.to_string()),
        "the refusal locates the unavailable range: {message}"
    );

    // The ranged form refuses too, and for the same reason: the requested
    // span sits inside the readable extent, but the file does not, and an
    // extracted file is whole or it is nothing.
    let mut buf = [0u8; 512];
    let ranged = fs(medium, partition)
        .get_file(FAR)
        .expect("the directory record is inside the readable extent")
        .read_at(0, &mut buf)
        .expect_err("a range of an unavailable file is refused");
    assert_eq!(ranged.category(), ErrorCategory::Unavailable);
    assert_eq!(
        ranged.rule(),
        Some(AssuranceCondition::SourceTruncated.as_str())
    );
    assert_eq!(buf, [0u8; 512], "nothing was served into the buffer");

    drop(session);
    std::fs::remove_file(&path).ok();
}

#[test]
fn a_read_of_the_medium_past_the_extent_names_the_condition() {
    let (path, mut session, attachment) = truncated("medium-read", Afford::Read);
    let medium = session.medium_mut(attachment).expect("medium");

    let mut inside = [0u8; 512];
    medium
        .read_at(OBSERVED - 512, &mut inside)
        .expect("the last present sector reads");

    let mut crossing = [0u8; 512];
    let refusal = medium
        .read_at(OBSERVED - 256, &mut crossing)
        .expect_err("a read crossing the extent is refused");
    assert_eq!(refusal.category(), ErrorCategory::Unavailable);
    assert_eq!(
        refusal.rule(),
        Some(AssuranceCondition::SourceTruncated.as_str())
    );

    drop(session);
    std::fs::remove_file(&path).ok();
}

#[test]
fn a_source_too_short_for_the_leading_structures_says_so_and_still_inspects() {
    // Truncated inside the first FAT copy: the bound is exact, so this is
    // degraded rather than refused, but nothing is addressable through it
    // and the assurance says which structure is missing before a verb has
    // to fail.
    let path = temp_path("no-metadata");
    build_floppy(&path);
    truncate(&path, 4_096);

    let mut session = Session::new();
    let medium = session
        .load_media(
            open_read(&path),
            Format::Raw {
                device: HardDrive::MbrSector.into(),
                block_bytes: 512,
            },
        )
        .expect("what is left still loads");
    let assurance = medium.assurance().clone();
    assert_eq!(assurance.outcome, AssuranceOutcome::Degraded);
    assert_eq!(assurance.observed_bytes, Some(4_096));
    assert!(
        assurance
            .evidence
            .iter()
            .any(|line| line.contains("no entry is addressable")),
        "{:?}",
        assurance.evidence
    );

    // The layered report is still answered: a filesystem whose structures
    // are gone is an outcome carried as that seam's issue, not a failure
    // to inspect the device.
    let report = medium.inspect().expect("inspection still reads");
    let issue = report
        .filesystems
        .first()
        .and_then(|filesystem| filesystem.issues.first())
        .expect("the filesystem seam reports the shortfall as its issue");
    assert_eq!(issue.category(), ErrorCategory::Unavailable);
    assert_eq!(
        issue.rule(),
        Some(AssuranceCondition::SourceTruncated.as_str())
    );

    // And the node answers the same way one seam up. **An absent
    // namespace is not a failure to reach the space** — which is no
    // longer a claim made beside the door but the door's own semantics:
    // `volume()` is a lookup, answering because the extent was settled
    // when the pool was established, and it answers whether or not
    // anything declares a namespace over the partition (P19, D26).
    let volume = report.volumes[0].id;
    let ordinal = only_partition_of(medium);
    assert_eq!(
        medium.partitions()[0].volume_id(),
        Some(volume),
        "and the partition carries the very identity the report issued (P21, U4)"
    );
    let mut space = medium
        .partition(ordinal)
        .expect("the pool bears the direct partition")
        .volume()
        .expect("which composes the whole content as one extent");
    assert!(
        space.is_addressable(),
        "the direct partition composed an extent"
    );
    assert!(!space.has_namespace(), "and nothing declares one over it");
    let refusal = space.kind().expect_err("no namespace is addressable");
    assert_eq!(
        refusal.rule(),
        Some(PartitionRule::UnclaimedNamespace.as_str()),
        "the absence is this seam's own, and names the declaration that \
         would settle it: {refusal}"
    );
    let listing = space.entries("").expect_err("and the verbs answer alike");
    assert_eq!(
        listing.rule(),
        refusal.rule(),
        "one absence, however it is met"
    );

    // The space holds its borrow of what it reads through until it is
    // dropped, so the medium is free for the next door only once this one
    // is done with it.
    drop(space);

    // The declared reading is where the recognition seam actually runs,
    // and its refusal surfaces there with category and rule intact rather
    // than as a coarser absence of this seam's own: the FAT adapter is
    // asked to verify the declaration and answers that the structures it
    // would read are past the readable extent.
    let declared = medium
        .partition(ordinal)
        .expect("the pool bears the direct partition")
        .filesystem_as("fat")
        .expect_err("the structures a FAT reading needs are gone");
    assert_eq!(declared.category(), ErrorCategory::Unavailable);
    assert_eq!(
        declared.rule(),
        Some(AssuranceCondition::SourceTruncated.as_str()),
        "the recognizing seam's own rule, not one this seam invented: {declared}"
    );

    // The session outlives what borrows it rather than the other way
    // round.
    drop(session);
    std::fs::remove_file(&path).ok();
}

#[test]
fn contradictory_metadata_beneath_a_shortfall_is_refused_not_degraded() {
    let path = temp_path("conflict");
    let mut image = floppy_1440();
    // Both total-sector fields filled, with different counts: the record
    // declares two sizes and settles neither, so no bound can be stated
    // for the shortfall observed beside it.
    image[32..36].copy_from_slice(&5_760u32.to_le_bytes());
    std::fs::write(&path, image).expect("image writes");
    truncate(&path, OBSERVED);

    let mut session = Session::new();
    let refusal = session
        .load_media(
            open_read(&path),
            Format::Raw {
                device: HardDrive::MbrSector.into(),
                block_bytes: 512,
            },
        )
        .expect_err("an unbounded shortfall is refused at the load");
    assert_eq!(refusal.category(), ErrorCategory::InvalidImage);
    assert_eq!(
        refusal.rule(),
        Some(AssuranceCondition::EvidenceConflict.as_str())
    );
    let message = refusal.to_string();
    assert!(
        message.contains("2880") && message.contains("5760"),
        "the refusal names both declarations: {message}"
    );

    std::fs::remove_file(&path).ok();
}

#[test]
fn the_gate_is_narrow_and_claims_no_rule_beyond_the_raw_direct_volume() {
    // A truncated partitioned disk: the leading sector is a partition
    // table, not a boot record, and no automatic MBR degradation rule is
    // claimed. The open is verified, and what the missing tail costs
    // shows up where it always did.
    let path = temp_path("narrow");
    std::fs::write(&path, mbr_disk(&floppy_1440())).expect("image writes");
    truncate(&path, 1_100_000);

    let mut session = Session::new();
    let medium = session
        .load_media(
            open_write(&path),
            Format::Raw {
                device: HardDrive::MbrSector.into(),
                block_bytes: 512,
            },
        )
        .expect("the image loads");
    assert_eq!(medium.assurance().outcome, AssuranceOutcome::Verified);
    assert_eq!(medium.assurance().condition, None);
    assert_eq!(
        medium.mode(),
        AccessMode::ReadWrite,
        "nothing narrowed this session's authority"
    );

    drop(session);
    std::fs::remove_file(&path).ok();
}
