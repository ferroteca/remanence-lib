// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The KryoFlux capture-set adapter over the prepared fixture: one disk
//! spread over a stream per head per drive-step position, opened through
//! the 7z catalog and recognized as one capture.
//!
//! What this asserts is preservation. Every member's transfer survives
//! with the flux recorded before its first index and after its last; the
//! index records become the timed markers the circular observations are
//! bounded at; the two heads stay two locations and are never merged
//! into one ideal disk; and the whole capture is addressed out of
//! private session storage rather than held (P27). A set that does not
//! hold together is refused by name, with the catalog evidence that
//! refused it.

use std::path::PathBuf;

use remanence::{CaptureSet, ErrorCategory};

mod common;

const ARCHIVE: &str = "Bill Budge Pinball Construction Set [Commodore 64] (1of2).7z";
/// 84 drive-step positions, each captured by both heads.
const STEP_COUNT: u64 = 84;
const HEADS: u64 = 2;
const MEMBER_COUNT: usize = (STEP_COUNT * HEADS) as usize;

/// The KryoFlux sample clock, exactly: `((18432000 * 73) / 14) / 2 / 2`
/// hertz. The stream's own `sck` is a decimal truncation of it.
const SAMPLE_CLOCK_NUMERATOR: u64 = 18_432_000 * 73;
const SAMPLE_CLOCK_DENOMINATOR: u64 = 56;

/// Six index records per member, so five whole revolutions.
const OBSERVATIONS_PER_MEMBER: usize = 5;

/// The set holds the P7 deny-write claim for its lifetime, so tests
/// opening the fixture concurrently take private copies.
fn private_copy(tag: &str) -> PathBuf {
    let target = std::env::temp_dir().join(format!(
        "remanence-kryoflux-{tag}-{}.7z",
        std::process::id()
    ));
    std::fs::copy(common::ensure_fixture(ARCHIVE), &target).expect("fixture copies");
    target
}

#[test]
fn the_whole_set_is_recognized_as_one_capture() {
    let path = private_copy("set");
    let set = CaptureSet::open(&path).expect("the archive holds one whole capture set");

    assert_eq!(set.format_id(), "kryoflux");
    assert_eq!(set.format_name(), "KryoFlux capture set");
    assert_eq!(set.archive_format_id(), "7z");

    let report = set.inspect();
    assert_eq!(report.members.len(), MEMBER_COUNT);
    assert_eq!(
        report.time_base.ticks_per_second_numerator,
        SAMPLE_CLOCK_NUMERATOR
    );
    assert_eq!(
        report.time_base.ticks_per_second_denominator,
        SAMPLE_CLOCK_DENOMINATOR
    );
    assert!(
        report
            .evidence
            .iter()
            .any(|line| line.contains("168 KryoFlux stream members")),
        "{:?}",
        report.evidence
    );

    // Every position by every head, in the set's own addressing order,
    // each named by the catalog identity it was read from.
    for (index, member) in report.members.iter().enumerate() {
        let (step, head) = (index as u64 / HEADS, index as u64 % HEADS);
        assert_eq!(member.position.numerator, step);
        assert_eq!(member.position.denominator, 1);
        assert_eq!(member.head, Some(head));
        assert_eq!(
            member.entry_name,
            format!(
                "Bill Budge Pinball Construction Set[Commodore 64](1of2){step:02}.{head}.raw"
            )
        );
        assert!(member.issues.is_empty(), "{:?}", member.issues);
    }

    drop(set);
    std::fs::remove_file(&path).ok();
}

#[test]
fn every_members_transfer_survives_whole_with_its_markers() {
    let path = private_copy("runs");
    let set = CaptureSet::open(&path).expect("the set opens");

    for member in &set.inspect().members {
        // One transfer per member, which is what one stream file is.
        assert_eq!(member.runs.len(), 1, "{}", member.entry_name);
        let run = &member.runs[0];
        assert_eq!(run.ordinal, 0);
        assert_eq!(run.transfer_result, Some(0), "{}", member.entry_name);

        // The index records are the whole of the marker channel here,
        // and six of them bound five circular observations.
        assert_eq!(run.index_markers, 6, "{}", member.entry_name);
        assert_eq!(run.markers, run.index_markers, "{}", member.entry_name);
        assert_eq!(
            run.observations.len(),
            OBSERVATIONS_PER_MEMBER,
            "{}",
            member.entry_name
        );

        // Flux before the first index is retained rather than consumed
        // by the bounding: the capture brackets its revolutions, and
        // what sits outside them is still evidence.
        assert!(
            run.transitions_before_first_index > 0,
            "{} recorded nothing before its first index",
            member.entry_name
        );
        let bounded: u64 = run
            .observations
            .iter()
            .map(|observation| observation.transitions)
            .sum();
        assert_eq!(
            bounded + run.transitions_before_first_index + run.transitions_after_last_index,
            run.transitions,
            "{} loses transitions between its run and its observations",
            member.entry_name
        );

        // Each observation is one revolution of the 360 RPM instrument
        // the disk was read on, and states its own circumference rather
        // than inheriting a nominal one.
        for observation in &run.observations {
            assert!(
                (3_900_000..4_100_000).contains(&observation.span_ticks),
                "{} observation {} spans {} ticks",
                member.entry_name,
                observation.ordinal,
                observation.span_ticks
            );
        }
        let ordinals: Vec<u64> = run
            .observations
            .iter()
            .map(|observation| observation.ordinal)
            .collect();
        assert_eq!(ordinals, [0, 1, 2, 3, 4]);
    }

    drop(set);
    std::fs::remove_file(&path).ok();
}

#[test]
fn the_two_heads_stay_two_locations_and_are_never_merged() {
    let path = private_copy("heads");
    let set = CaptureSet::open(&path).expect("the set opens");
    let report = set.inspect();

    // At step 0 head 0 carries the recording and head 1 is the
    // unrecorded back of a single-sided disk. The evidence says so on
    // its own: the recorded side reproduces transition for transition
    // across every revolution, while the noise varies by hundreds.
    // Telling the two apart is not this layer's job — keeping them
    // apart, so that something above can, is.
    let front = &report.members[0];
    let back = &report.members[1];
    assert_eq!(front.head, Some(0));
    assert_eq!(back.head, Some(1));
    assert_eq!(front.position.numerator, back.position.numerator);

    let spread = |member: &remanence::CaptureSetMember| {
        let counts: Vec<u64> = member.runs[0]
            .observations
            .iter()
            .map(|observation| observation.transitions)
            .collect();
        counts.iter().max().copied().unwrap_or(0) - counts.iter().min().copied().unwrap_or(0)
    };
    assert!(
        spread(front) < 8,
        "the recorded side varies by {} transitions between passes",
        spread(front)
    );
    assert!(
        spread(back) > 100,
        "the unrecorded side varies by only {} transitions between passes",
        spread(back)
    );

    // Nothing anywhere in the set reports one location for a position:
    // every step appears twice, once per head.
    for step in 0..STEP_COUNT {
        let at_step: Vec<_> = report
            .members
            .iter()
            .filter(|member| member.position.numerator == step)
            .collect();
        assert_eq!(at_step.len(), HEADS as usize);
        assert_eq!(at_step[0].head, Some(0));
        assert_eq!(at_step[1].head, Some(1));
    }

    drop(set);
    std::fs::remove_file(&path).ok();
}

#[test]
fn the_capture_is_addressed_out_of_session_storage_rather_than_held() {
    let path = private_copy("bounded");
    // A working set far smaller than the capture: the whole set still
    // opens, and what stays resident stays inside the bound (P27).
    let bound = 256 * 1024;
    let set = CaptureSet::open_with_cache(&path, bound).expect("the set opens under the bound");

    assert_eq!(set.inspect().members.len(), MEMBER_COUNT);
    assert!(
        set.backing_bytes() > 16 * 1024 * 1024,
        "the decoded capture is {} bytes",
        set.backing_bytes()
    );
    assert!(
        set.resident_bytes() <= bound,
        "{} bytes resident against a {bound}-byte bound",
        set.resident_bytes()
    );

    drop(set);
    std::fs::remove_file(&path).ok();
}

#[test]
fn a_path_naming_no_catalog_is_refused_rather_than_read_as_one_member() {
    // The logical capture is the whole set, so a lone file is not a
    // small capture set — it is not one at all.
    let error = CaptureSet::open("nowhere/capture00.0.raw")
        .expect_err("a stream file is not a capture set");
    assert_eq!(error.category(), ErrorCategory::Unsupported);
    assert!(
        error.to_string().contains("names no archive"),
        "{error}"
    );
}

/// A minimal KryoFlux stream: the device information, `cells` of
/// one-byte flux values, index records naming stream positions inside
/// them, the declared transfer result, and the end marker.
fn kryoflux_stream(cells: &[u8], indices: &[(u32, u32)], result: u32) -> Vec<u8> {
    const OOB: u8 = 0x0d;
    let mut out = Vec::new();
    let info = b"sck=24027428.5714285, ick=3003428.5714285625\0";
    out.extend_from_slice(&[OOB, 0x04]);
    out.extend_from_slice(&(info.len() as u16).to_le_bytes());
    out.extend_from_slice(info);
    for (position, counter) in indices {
        out.extend_from_slice(&[OOB, 0x02]);
        out.extend_from_slice(&12u16.to_le_bytes());
        out.extend_from_slice(&position.to_le_bytes());
        out.extend_from_slice(&counter.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
    }
    out.extend_from_slice(cells);
    out.extend_from_slice(&[OOB, 0x03]);
    out.extend_from_slice(&8u16.to_le_bytes());
    out.extend_from_slice(&(cells.len() as u32).to_le_bytes());
    out.extend_from_slice(&result.to_le_bytes());
    out.extend_from_slice(&[OOB, OOB, OOB, OOB]);
    out
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xedb8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

/// A ZIP of stored entries, in the order given.
fn stored_zip(entries: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut directory = Vec::new();
    for (name, data) in entries {
        let offset = out.len() as u32;
        let (crc, size) = (crc32(data), data.len() as u32);
        out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        out.extend_from_slice(&[20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(data);

        directory.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        directory.extend_from_slice(&[20, 0, 20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        directory.extend_from_slice(&crc.to_le_bytes());
        directory.extend_from_slice(&size.to_le_bytes());
        directory.extend_from_slice(&size.to_le_bytes());
        directory.extend_from_slice(&(name.len() as u16).to_le_bytes());
        // Extra field, comment, disk start, internal and external
        // attributes: all absent or zero.
        directory.extend_from_slice(&[0u8; 12]);
        directory.extend_from_slice(&offset.to_le_bytes());
        directory.extend_from_slice(name.as_bytes());
    }
    let (start, length) = (out.len() as u32, directory.len() as u32);
    out.extend_from_slice(&directory);
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&[0, 0, 0, 0]);
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&length.to_le_bytes());
    out.extend_from_slice(&start.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

/// A capture set of `steps` positions by two heads, written as a ZIP.
fn synthetic_set(steps: u64, result: u32) -> Vec<(String, Vec<u8>)> {
    let mut entries = Vec::new();
    for step in 0..steps {
        for head in 0..2 {
            entries.push((
                format!("synthetic{step:02}.{head}.raw"),
                // Transitions at ticks 20, 60, 90, 140 and 210, with
                // the indices at 60 and 140: one transition before the
                // first, two from the last onward, and one revolution
                // bracketed between them.
                kryoflux_stream(&[0x14, 0x28, 0x1e, 0x32, 0x46], &[(2, 0), (4, 0)], result),
            ));
        }
    }
    entries
}

fn temp_zip(tag: &str, bytes: &[u8]) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "remanence-kryoflux-{tag}-{}.zip",
        std::process::id()
    ));
    std::fs::write(&path, bytes).expect("the archive writes");
    path
}

#[test]
fn the_adapter_reads_any_catalog_the_library_claims() {
    // The capture-set grammar sits above the archive catalog and knows
    // nothing about which one it is: the same set read out of a ZIP is
    // the same set. Nothing here is 7z-specific.
    let path = temp_zip("zip", &stored_zip(&synthetic_set(2, 0)));
    let set = CaptureSet::open(&path).expect("a ZIP holds a capture set as readily as a 7z");

    assert_eq!(set.archive_format_id(), "zip");
    let report = set.inspect();
    assert_eq!(report.members.len(), 4);
    assert_eq!(report.members[3].entry_name, "synthetic01.1.raw");
    assert_eq!(report.members[3].head, Some(1));

    let run = &report.members[0].runs[0];
    assert_eq!(run.transitions, 5);
    assert_eq!(run.extent_ticks, 210);
    assert_eq!(run.index_markers, 2);
    assert_eq!(run.transitions_before_first_index, 1);
    assert_eq!(run.transitions_after_last_index, 2);
    assert_eq!(run.transfer_result, Some(0));

    // Two indices bracket one revolution, which states the
    // circumference they measured rather than a nominal one.
    assert_eq!(run.observations.len(), 1);
    assert_eq!(run.observations[0].span_ticks, 80);
    assert_eq!(run.observations[0].transitions, 2);

    drop(set);
    std::fs::remove_file(&path).ok();
}

#[test]
fn a_transfer_the_tool_did_not_call_clean_is_reported_not_repaired() {
    let path = temp_zip("result", &stored_zip(&synthetic_set(1, 2)));
    let set = CaptureSet::open(&path).expect("the member still decodes");

    let member = &set.inspect().members[0];
    assert_eq!(member.runs[0].transfer_result, Some(2));
    assert_eq!(member.issues[0].code, "kryoflux-transfer-result");
    assert!(member.issues[0].detail.contains("transfer result 2"), "{:?}", member.issues);

    drop(set);
    std::fs::remove_file(&path).ok();
}

#[test]
fn a_set_missing_one_head_of_one_position_is_refused_with_the_evidence() {
    let mut entries = synthetic_set(2, 0);
    entries.remove(3);
    let path = temp_zip("incomplete", &stored_zip(&entries));

    let error = CaptureSet::open(&path).expect_err("a set with a hole in it is not one capture");
    assert_eq!(error.category(), ErrorCategory::InvalidImage);
    let message = error.to_string();
    assert!(message.contains("step position 1 head 1 is absent"), "{message}");
    assert!(message.contains("3 members"), "{message}");

    std::fs::remove_file(&path).ok();
}

#[test]
fn an_archive_holding_no_stream_members_is_refused_by_name() {
    let path = std::env::temp_dir().join(format!(
        "remanence-kryoflux-empty-{}.zip",
        std::process::id()
    ));
    std::fs::copy(
        common::ensure_fixture("HDOS_1-0_Issue_#50-00-00_890-1.zip"),
        &path,
    )
    .expect("fixture copies");

    let error = CaptureSet::open(&path).expect_err("a disk image is not a capture set");
    assert_eq!(error.category(), ErrorCategory::InvalidImage);
    assert!(
        error.to_string().contains("is not a KryoFlux stream member"),
        "{error}"
    );

    std::fs::remove_file(&path).ok();
}
