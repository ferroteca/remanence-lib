// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! Presenting a reconstructed disk as a 1541 hardware bitstream, then
//! as the family's encoded bytestream, then as the sectors the
//! recording states for itself — and finally as the directory CBM DOS
//! wrote across them.
//!
//! The claim under test is P23's, P33's and P30's together: the layers above a
//! flux medium are materialized under rules the drive profile declares,
//! each transition carries what produced everything beneath it, and
//! neither of the two layers below the sector one assigns
//! synchronization, headers, sectors or files to what it holds. The
//! third rung is where that ends, and it ends by stating what it
//! derives: every record it recognizes carries its evidence, and a
//! sector that does not read is a named refusal rather than a filled
//! block (P4, P10).
//!
//! The fourth rung is reached the way every namespace is: through the
//! partition that composes it (P19). A recording records no partition
//! scheme, so the sector layer bears the **direct partition** — the
//! library's own composition of the whole recording, carried as
//! provenance and never as evidence, and extent-less because a
//! recording's blocks are addressed by the recording rather than by
//! position (P13). Nothing there determines a reading, so the CBM DOS
//! one is *declared* over it and the recognizer verifies that
//! declaration rather than picking one (P18).
//!
//! The journey is the real one — a KryoFlux capture of a C64 disk,
//! reduced gap-first to the remanence image and then read by a declared
//! drive — so the numbers below are what a 1541 makes of an actual
//! recording rather than what a synthetic one was built to produce.
//!
//! The image carries no clock, so the ladder stands on the served
//! projection of it rather than on the image directly: one multiply per
//! point, at the family's reference frame.

use std::sync::OnceLock;

use remanence::{
    AlignmentPolicy, BitstreamReport, BytestreamReport, C1541Sectors, CaptureSet,
    ChecksumFailurePolicy, DensityPolicy, ErrorCategory, GcrCodecPolicy, PartitionRule,
    ReadChannelPolicy, ReconstructionPolicy, RecordingSelection, SectorPolicy, SectorRule,
    UnassignedSymbolPolicy, UnpairedRecordPolicy, UnzonedPolicy, WeakPulsePolicy,
};

mod common;

const ARCHIVE: &str = "Bill Budge Pinball Construction Set [Commodore 64] (1of2).7z";
/// The 1541's reference clock across one 300 RPM rotation.
const CYCLES_PER_ROTATION: u64 = 3_200_000;
/// The recordings the reduction admits from a whole side, the fat
/// track merged rather than asserted.
const LOCATIONS: usize = 36;

fn reconstruction() -> ReconstructionPolicy {
    ReconstructionPolicy {
        side: 0,
        recordings: RecordingSelection::Measured,
    }
}

fn channel() -> ReadChannelPolicy {
    ReadChannelPolicy {
        density: DensityPolicy::Declared,
        unzoned: UnzonedPolicy::Refuse,
        weak_pulse: WeakPulsePolicy::Seeded,
        seed: 0x0123_4567_89ab_cdef,
    }
}

fn codec() -> GcrCodecPolicy {
    GcrCodecPolicy {
        alignment: AlignmentPolicy::Landmark,
        unassigned_symbol: UnassignedSymbolPolicy::DeclareLoss,
    }
}

fn grammar() -> SectorPolicy {
    SectorPolicy {
        checksum_failure: ChecksumFailurePolicy::DeclareLoss,
        unpaired_record: UnpairedRecordPolicy::DeclareLoss,
    }
}

struct Presented {
    bitstream: BitstreamReport,
    /// The same transition run twice from the same medium and policy.
    repeated: BitstreamReport,
    bitstream_backing: u64,
    bitstream_resident: u64,
    bytestream: BytestreamReport,
    bytestream_backing: u64,
    bytestream_resident: u64,
    /// What the codec said when the policy refused a pattern the
    /// family's table does not assign.
    unassigned: Option<(ErrorCategory, String)>,
    /// The third rung, which outlives the bytestream it was recognized
    /// out of: its payloads are its own session state.
    sectors: C1541Sectors,
    /// What the recognition said when the policy refused a block whose
    /// stated checksum its own bytes do not compute.
    failed_checksum: Option<(ErrorCategory, Option<String>, String)>,
}

fn presented() -> &'static Presented {
    static PRESENTED: OnceLock<Presented> = OnceLock::new();
    PRESENTED.get_or_init(|| {
        let path =
            std::env::temp_dir().join(format!("remanence-presentation-{}.7z", std::process::id()));
        std::fs::copy(common::ensure_fixture(ARCHIVE), &path).expect("fixture copies");
        let set = CaptureSet::open(&path).expect("the set opens");
        let image = set
            .plan_reconstruction(&reconstruction())
            .expect("the reduction plans")
            .execute(1 << 20)
            .expect("the image is produced");

        let bitstream = image
            .materialize_c1541_bitstream(channel(), 1 << 20)
            .expect("the channel clocks the medium");
        let repeated = image
            .materialize_c1541_bitstream(channel(), 1 << 20)
            .expect("and clocks it again");
        let bytestream = bitstream
            .materialize_c1541_bytestream(codec(), 1 << 20)
            .expect("the codec resolves the bitstream");
        let unassigned = bitstream
            .materialize_c1541_bytestream(
                GcrCodecPolicy {
                    unassigned_symbol: UnassignedSymbolPolicy::Refuse,
                    ..codec()
                },
                1 << 20,
            )
            .err()
            .map(|error| (error.category(), error.to_string()));

        let sectors = bytestream
            .recognize_c1541_sectors(grammar(), 1 << 20)
            .expect("the family's record grammar reads the recording's own sectors");
        let failed_checksum = bytestream
            .recognize_c1541_sectors(
                SectorPolicy {
                    checksum_failure: ChecksumFailurePolicy::Refuse,
                    ..grammar()
                },
                1 << 20,
            )
            .err()
            .map(|error| {
                (
                    error.category(),
                    error.rule().map(str::to_owned),
                    error.to_string(),
                )
            });

        let presented = Presented {
            bitstream: bitstream.inspect().clone(),
            repeated: repeated.inspect().clone(),
            bitstream_backing: bitstream.backing_bytes(),
            bitstream_resident: bitstream.resident_bytes(),
            bytestream: bytestream.inspect().clone(),
            bytestream_backing: bytestream.backing_bytes(),
            bytestream_resident: bytestream.resident_bytes(),
            unassigned,
            sectors,
            failed_checksum,
        };
        drop(bytestream);
        drop(repeated);
        drop(bitstream);
        drop(image);
        drop(set);
        std::fs::remove_file(&path).ok();
        presented
    })
}

#[test]
fn every_location_is_clocked_at_the_cell_its_declared_zone_states() {
    let report = &presented().bitstream;

    assert_eq!(report.profile_id, "c1541");
    assert_eq!(report.profile_name, "Commodore 1541");
    assert_eq!(report.reference_clock_hz, 16_000_000);
    assert_eq!(report.cycles_per_rotation, CYCLES_PER_ROTATION);
    assert_eq!(report.locations.len(), LOCATIONS);

    // The four documented zones, at their documented cells: the tracks
    // this capture holds fall in the first two, and each is clocked at
    // the rate its own zone declares rather than at one rate throughout.
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

    for location in &report.locations {
        assert_eq!(location.cell_cycles_denominator, 1);
        // Whole tracks address as n/1; the half-tracks between them
        // address as n/2, and the reduction admits one where the
        // evidence shows a recording there rather than only where the
        // 35-track grid expects one.
        assert!(
            matches!(location.half_track_denominator, 1 | 2),
            "{location:?}"
        );
        assert_eq!(location.surface, Some(0));
        // About one rotation's worth of cells — and only about, which is
        // the point: the channel locks onto the recording's own phase,
        // so a disk written a little fast holds a few more bits than the
        // nominal rate states rather than being read as though it did
        // not.
        let nominal = CYCLES_PER_ROTATION / location.cell_cycles_numerator;
        assert!(
            location.cells * 20 > nominal * 19 && location.cells * 20 < nominal * 21,
            "{location:?} against a nominal {nominal}"
        );
        assert!(location.longest_zero_run >= 1, "{location:?}");
        assert_eq!(location.recorded_bits + location.resolved_bits, location.cells);
        // The medium's pulses all carry the strength its policy declared,
        // so nothing here was resolved by a rule instead of read.
        assert_eq!(location.resolved_bits, 0, "{location:?}");
        assert!(location.one_bits > 10_000, "{location:?}");
        // And the circle does not divide into cells: what is left over
        // is stated rather than rounded into a bit.
        assert!(
            location.wrap_slack_numerator < location.cell_cycles_numerator,
            "{location:?}"
        );
    }

    // GCR admits at most two zeros between transitions, so a disk whose
    // clocked tracks held long runs everywhere would be saying the
    // channel was reading at the wrong cell. Two claims hold here, and
    // the second is the load-bearing one: most locations hold exactly
    // the runs the encoding permits, and *every* departure is by a cell
    // or two rather than by an order of magnitude. A channel locked to
    // the wrong cell shows runs of tens; these are threes and fours,
    // and they cluster on the fat track's fringe reads — the same
    // recording picked up at a radius the head only partly covers. A
    // location that departs is reported as the number it is rather than
    // smoothed into the others.
    let longest = report
        .locations
        .iter()
        .map(|location| location.longest_zero_run)
        .max()
        .expect("a whole side holds locations");
    assert!(
        longest <= 4,
        "the channel is reading at the recording's own cell: longest zero run {longest}"
    );
    let departing: Vec<u64> = report
        .locations
        .iter()
        .filter(|location| location.longest_zero_run > 2)
        .map(|location| location.half_track_numerator)
        .collect();
    assert!(
        departing.len() * 2 < report.locations.len(),
        "{departing:?} of {} locations depart from the encoding",
        report.locations.len()
    );
}

#[test]
fn the_bitstream_states_what_it_does_not_carry_of_the_medium() {
    let report = &presented().bitstream;

    // A count is not an account here either: every entry says what was
    // not carried, in the terms the medium stated it.
    for loss in &report.declared_loss {
        assert!(!loss.code.is_empty());
        assert!(loss.detail.len() > 20, "{loss:?}");
        assert!(loss.count > 0, "{loss:?}");
    }
    // The seam each location's reduction located is carried up as the
    // angle it is; a duplicate the caller admitted would not be, and
    // neither would the medium's write-protect state.
    assert!(
        report
            .declared_loss
            .iter()
            .all(|loss| loss.code != "unexpressed-pulse-strength"),
        "{:?}",
        report.declared_loss
    );

    // The channel that produced it and the policy that produced the
    // medium are both stated, in that order.
    assert!(report.evidence.len() >= 5, "{:?}", report.evidence);
    assert!(report.evidence[0].contains("Commodore 1541"), "{:?}", report.evidence);
    assert!(
        report
            .evidence
            .iter()
            .any(|line| line.contains("the medium beneath it")),
        "{:?}",
        report.evidence
    );
    assert!(
        report
            .evidence
            .iter()
            .any(|line| line.contains("restarts at every detected transition")
                && line.contains("1/2 of a cell")),
        "{:?}",
        report.evidence
    );
}

#[test]
fn the_same_medium_and_policy_produce_the_same_bitstream() {
    assert_eq!(presented().bitstream, presented().repeated);
}

#[test]
fn the_codec_frames_on_the_declared_landmark_and_claims_nothing_past_it() {
    let report = &presented().bytestream;

    assert_eq!(report.profile_id, "c1541");
    assert_eq!(report.codec_id, "c1541-gcr");
    assert_eq!(report.symbol_bits, 5);
    assert_eq!(report.data_bits, 4);
    assert_eq!(report.symbols_per_byte, 2);
    assert_eq!(report.locations.len(), LOCATIONS);

    for location in &report.locations {
        // A 1541 writes a sync ahead of each header and each data field,
        // so a track's landmark count runs with its sector count. What
        // the layer states is that it found them, and nothing about what
        // any of them introduces.
        assert!(location.alignments >= 30, "{location:?}");
        assert!(location.longest_landmark_bits >= 10, "{location:?}");
        assert!(location.bytes > 4_000, "{location:?}");
        assert_eq!(
            location.resolved_bytes + location.unassigned_groups,
            location.bytes
        );
        // Most of the recording resolves through the family's own table.
        assert!(
            location.resolved_bytes * 10 > location.bytes * 9,
            "{location:?}"
        );
    }
}

#[test]
fn a_pattern_the_family_assigns_nothing_to_is_refused_or_kept_as_its_bits() {
    // Which of the two happens is the caller's declaration, and the
    // refusing one names the location and the bit rather than reporting
    // a value the recording never held.
    let refused = presented()
        .unassigned
        .as_ref()
        .expect("a real recording holds patterns the table does not assign");
    assert_eq!(refused.0, ErrorCategory::InvalidImage);
    assert!(refused.1.contains("does not assign"), "{}", refused.1);
    assert!(refused.1.contains("never recorded"), "{}", refused.1);

    let report = &presented().bytestream;
    let by_code = |code: &str| {
        report
            .declared_loss
            .iter()
            .find(|loss| loss.code == code)
            .unwrap_or_else(|| panic!("{code} is not accounted for: {:?}", report.declared_loss))
    };
    // The landmark's own bits frame the bytes rather than becoming one,
    // the bits no group covers are stated, and the groups the table
    // assigns nothing to keep what they held.
    by_code("alignment-landmark");
    by_code("unframed-bits");
    by_code("unassigned-symbol");

    assert!(
        report
            .evidence
            .iter()
            .any(|line| line.contains("no byte here is a header")),
        "{:?}",
        report.evidence
    );
    // And the whole chain beneath it is still readable two layers up.
    assert!(
        report
            .evidence
            .iter()
            .any(|line| line.contains("served projection of a remanence image")),
        "{:?}",
        report.evidence
    );
}

#[test]
fn no_layer_is_held_whole_in_memory() {
    // Each is addressed out of private session storage under a declared
    // bound (P27), and producing them made none of them resident.
    let presented = presented();
    assert!(presented.bitstream_backing > 0);
    assert!(presented.bytestream_backing > 0);
    assert!(presented.sectors.backing_bytes() > 0);
    assert!(
        presented.bitstream_resident <= 1 << 20,
        "{} bytes resident",
        presented.bitstream_resident
    );
    assert!(
        presented.bytestream_resident <= 1 << 20,
        "{} bytes resident",
        presented.bytestream_resident
    );
    assert!(
        presented.sectors.resident_bytes() <= 1 << 20,
        "{} bytes resident",
        presented.sectors.resident_bytes()
    );
}

/// The 35-track grid CBM DOS defines, as the family declares it: the
/// four zones with their track boundaries and sector counts.
const ZONES: [(u8, u8, u8); 4] = [(1, 17, 21), (18, 24, 19), (25, 30, 18), (31, 35, 17)];

#[test]
fn the_recording_states_its_own_sectors_and_the_family_declares_how_many() {
    let report = presented().sectors.inspect();

    assert_eq!(report.profile_id, "c1541");
    assert_eq!(report.grammar_id, "cbm-dos-record");
    assert_eq!(report.grammar_name, "CBM DOS sector record");
    assert_eq!(report.payload_bytes, 256);
    // Every location the bytestream held is read; a half-track holding
    // no record would be reported holding none rather than left out.
    assert_eq!(report.locations.len(), LOCATIONS);

    // The load-bearing claim: what the recording holds is what the
    // family's density map says it should, track by track, and the count
    // was never assumed — the headers were found and the zone's figure
    // is what they are compared against. Two locations of this disk
    // depart, and they depart as the numbers they are.
    let exact = report
        .locations
        .iter()
        .filter(|location| location.records_declared == Some(location.headers as u32))
        .count();
    assert!(
        exact + 2 >= report.locations.len(),
        "{} of {} locations hold exactly what their zone declares",
        exact,
        report.locations.len()
    );
    for location in &report.locations {
        assert!(location.records <= location.headers, "{location:?}");
        assert!(location.readable <= location.records, "{location:?}");
        // A departure is a location whose recording is not the ordinary
        // one, and it says so in counts rather than by being absent.
        if location.records_declared != Some(location.headers as u32) {
            assert!(
                location.runs_without_a_record > 0 || location.failed_checksum > 0,
                "{location:?}"
            );
        }
    }

    // The fat track is read at two step positions, so seventeen
    // addresses are stated twice. The layer reports that and resolves
    // none of it: whether the two agree is decided when one is read.
    assert!(!report.contested.is_empty(), "{:?}", report.contested);
    for contested in &report.contested {
        assert!(contested.readable_claims >= 2, "{contested:?}");
        // They agree here — one recording picked up at two radii — so
        // the address still answers rather than refusing.
        assert_eq!(
            presented()
                .sectors
                .read_sector(contested.track, contested.sector)
                .expect("two readings of one recording agree")
                .len(),
            256
        );
    }
}

#[test]
fn every_claim_carries_its_evidence_and_an_unreadable_one_names_its_rule() {
    let report = presented().sectors.inspect();
    assert!(report.claims.len() > 600, "{} claims", report.claims.len());

    let mut unreadable = 0;
    for claim in &report.claims {
        // The address is what the header states for itself, and the two
        // checksums stand beside each other rather than collapsing into
        // a verdict.
        assert_eq!(claim.surface, Some(0));
        assert_eq!(claim.readable, claim.rule.is_none());
        if claim.readable {
            assert!(claim.has_data, "{claim:?}");
            assert_eq!(claim.unresolved_bytes, 0, "{claim:?}");
            assert_eq!(
                claim.header_checksum_stated, claim.header_checksum_computed,
                "{claim:?}"
            );
            assert_eq!(
                claim.data_checksum_stated, claim.data_checksum_computed,
                "{claim:?}"
            );
            continue;
        }
        unreadable += 1;
        // And a claim that does not read names the rule that stands in
        // the way (P10) and says what was expected and found (P6).
        let rule = claim.rule.as_deref().expect("checked above");
        assert!(
            SectorRule::from_identity(rule).is_some(),
            "{rule} is not in the set"
        );
        assert!(claim.refusal.as_ref().is_some_and(|why| why.len() > 20), "{claim:?}");
    }
    assert!(
        unreadable > 0,
        "a real recording holds records that do not read"
    );

    // The account of what this layer does not carry of the bytestream:
    // the gaps and tails between records, and every record it could not
    // read, each stated as what it was rather than counted.
    let by_code = |code: &str| {
        report
            .declared_loss
            .iter()
            .find(|loss| loss.code == code)
            .unwrap_or_else(|| panic!("{code} is not accounted for: {:?}", report.declared_loss))
    };
    assert!(by_code("byte-outside-a-record").count > 1000);
    assert!(by_code("block-failed-checksum").count > 0);
    assert!(by_code("run-without-a-record").count > 0);
    for loss in &report.declared_loss {
        assert!(loss.detail.len() > 20, "{loss:?}");
        assert!(loss.count > 0, "{loss:?}");
    }
}

#[test]
fn a_sector_reads_by_the_address_the_recording_states_for_it() {
    let sectors = &presented().sectors;

    // The disk's own block-availability map, read out of flux through
    // four layers: it links to track 18 sector 1 and states CBM DOS
    // version 'A', which is what a C64 would find there.
    let bam = sectors.read_sector(18, 0).expect("track 18 sector 0 reads");
    assert_eq!(bam.len(), 256);
    assert_eq!(&bam[..3], &[18, 1, 0x41]);

    // Most of the 683-block grid the recording defines comes back, and
    // every address that does not is a refusal naming its rule rather
    // than a block of zeros.
    let mut read = 0u32;
    let mut not_stated = 0u32;
    let mut not_readable = 0u32;
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
                        match SectorRule::from_identity(rule) {
                            Some(SectorRule::NoSuchAddress) => {
                                assert_eq!(error.category(), ErrorCategory::NotFound);
                                not_stated += 1;
                            }
                            Some(SectorRule::AddressNotReadable) => {
                                // The recording holds it and it does not
                                // read: imperfect evidence, not a host
                                // failure and not a contradiction.
                                assert_eq!(error.category(), ErrorCategory::Unavailable);
                                not_readable += 1;
                            }
                            other => panic!("{other:?} from {error}"),
                        }
                    }
                }
            }
        }
    }
    assert!(read > 640, "{read} of the grid's 683 blocks read");
    assert!(
        not_stated > 0 && not_readable > 0,
        "this disk states neither every block nor a readable one for every \
         address it does state: {not_stated} unstated, {not_readable} unreadable"
    );

    // A track the family's own density map does not reach is refused as
    // that rather than as a missing block.
    let error = sectors.read_sector(36, 0).expect_err("there is no track 36");
    assert_eq!(error.rule(), Some(SectorRule::NoSuchAddress.as_str()));
    assert!(error.to_string().contains("no track 36"), "{error}");
}

#[test]
fn the_recording_composes_the_direct_partition_and_its_namespace_is_declared() {
    let sectors = &presented().sectors;

    // A recording records no partition scheme, so the one member of its
    // pool is the library's own composition: ordinal zero, which no
    // scheme numbers, and an account that is provenance rather than
    // evidence — a composition act is stated as one and is never offered
    // as something the recording said (P4).
    let view = sectors.partition();
    assert!(view.is_direct());
    assert_eq!(view.ordinal(), 0);
    assert_eq!(view.type_byte(), None, "nothing here records a type");
    let direct = view.partition();
    assert_eq!(direct.placement(), "direct");
    assert!(
        direct
            .provenance()
            .is_some_and(|why| why.contains("never evidence")),
        "{direct:?}"
    );
    assert!(direct.evidence().is_empty(), "{direct:?}");

    // And it composes no addressed extent: a recording's blocks are
    // addressed by the recording rather than by position (P13), so there
    // is no position for an extent to be measured in. The addressable
    // vantage is absent rather than invented (D26) — a phantom volume
    // over a layer nothing addressed would be a claim the evidence does
    // not make.
    assert_eq!(direct.start_bytes(), None);
    assert_eq!(direct.length_bytes(), None);
    assert!(!direct.is_addressable());
    assert!(direct.volume_id().is_none());
    assert!(!direct.bears_namespace());
    assert!(
        view.volume().is_none(),
        "there is no extent for a position to be within"
    );

    // Nothing here determines a reading and this layer will not pick
    // one, so the plain door answers the honest absence P19 requires
    // rather than probing the blocks for a filesystem.
    assert!(sectors.partition().filesystem().is_none());

    // The reading is the caller's and the check is the library's: the
    // adapter the declaration names is the one that reads the directory
    // track, and this disk bears what was declared.
    let space = sectors
        .partition()
        .filesystem_as("cbmdos")
        .expect("this disk's directory track reads");
    assert_eq!(space.kind().expect("a namespace was recognized"), "cbmdos");

    // A namespace this release does read, declared over a layer it
    // cannot be read out of, is refused by name (P10) and says what *is*
    // read there rather than only that the answer is no.
    let error = sectors
        .partition()
        .filesystem_as("fat")
        .expect_err("a record layer bears no boot record to recognize FAT on");
    assert_eq!(error.rule(), Some(PartitionRule::UnclaimedNamespace.as_str()));
    assert_eq!(error.category(), ErrorCategory::Unsupported);
    assert!(error.to_string().contains("cbmdos"), "{error}");

    // And a spelling this release reads nowhere at all is refused the
    // same way, naming what it does read (P3).
    let error = sectors
        .partition()
        .filesystem_as("ext4")
        .expect_err("this release reads no ext4");
    assert_eq!(error.rule(), Some(PartitionRule::UnclaimedNamespace.as_str()));
    assert!(error.to_string().contains("ext4"), "names what was asked: {error}");
    assert!(error.to_string().contains("cbmdos"), "names what is read: {error}");
}

/// One entry's declared fact, by the key CBM DOS spells it with.
fn fact(entry: &remanence::Entry, key: &str) -> Option<String> {
    entry
        .declared
        .iter()
        .find(|fact| fact.key == key)
        .map(|fact| fact.value.clone())
}

#[test]
fn the_disk_bears_cbm_dos_and_the_bam_header_is_its_label() {
    let mut space = presented()
        .sectors
        .partition()
        .filesystem_as("cbmdos")
        .expect("this disk's directory track reads");

    assert_eq!(space.kind().expect("a namespace was recognized"), "cbmdos");
    // The namespace vantage and not the addressable one: nothing
    // composed an extent for this to be a position within, and the 0..1
    // is carried by what the node answers rather than asserted beside it.
    // The extent-lessness belongs to the direct partition this was
    // declared over and is stated there, at the composition that made it,
    // rather than only here at the space it handed out.
    assert!(!space.is_addressable());
    assert!(space.volume_id().is_none());

    // The BAM header, read as the label CBM DOS displays. This disk's
    // name is the autoboot trick — a PETSCII sequence that types
    // `LOAD"EA",8,1` when the directory is listed — so its unreadable
    // control bytes are marked in the reading and carried whole beside
    // it, which is the two-outcome rule doing exactly what it is for.
    let label = space
        .label()
        .expect("the namespace answers")
        .expect("a CBM DOS disk names itself");
    assert_eq!(label.answered_by.as_deref(), Some("bam-disk-name"));
    let reading = |source: &str| {
        label
            .readings
            .iter()
            .find(|reading| reading.source == source)
            .and_then(|reading| reading.stored.clone())
            .unwrap_or_else(|| panic!("{source} is not among {:?}", label.readings))
    };
    assert!(reading("bam-disk-name").contains("LOAD\"EA\",8,1"), "{label:?}");
    assert_eq!(
        reading("bam-disk-name-petscii"),
        "0d 93 4c 4f 41 44 22 45 41 22 2c 38 2c 31 0d 0d"
    );
    assert_eq!(reading("bam-disk-id"), "EA");
    assert_eq!(reading("bam-dos-type"), "2A");

    // And the claim carries what recognized it (P4).
    let evidence = space.evidence().expect("the namespace answers");
    assert!(
        evidence
            .iter()
            .any(|line| line.contains("DOS version byte 0x41")),
        "{evidence:?}"
    );
    assert!(
        evidence
            .iter()
            .any(|line| line.contains("every rule this reading applied is CBM DOS's")),
        "{evidence:?}"
    );
}

#[test]
fn the_directory_lists_in_its_own_order_with_the_facts_cbm_dos_records() {
    let mut space = presented()
        .sectors
        .partition()
        .filesystem_as("cbmdos")
        .expect("the disk bears it");
    let entries = space.entries("").expect("the directory chain reads");

    // Directory order is evidence (U4): the chain is walked from where
    // the BAM says it begins, and nothing is sorted on the way out.
    let names: Vec<&str> = entries.iter().map(|entry| entry.name.as_str()).collect();
    assert_eq!(names.len(), 16, "{names:?}");
    assert_eq!(names[0], "EA");
    assert_eq!(names[3], "DEMO1.PB");
    assert_eq!(names[15], "DEMO4.PB");
    // The chain crosses two directory blocks, and each entry says which
    // slot of which block it was read from.
    assert_eq!(fact(&entries[0], "directory-slot").as_deref(), Some("18/1#0"));
    assert_eq!(fact(&entries[8], "directory-slot").as_deref(), Some("18/4#0"));

    for entry in &entries {
        assert_eq!(entry.kind, remanence::EntryKind::File);
        assert_eq!(fact(entry, "type").as_deref(), Some("prg"));
        assert_eq!(fact(entry, "type-raw").as_deref(), Some("130"));
        assert_eq!(fact(entry, "closed").as_deref(), Some("true"));
        assert_eq!(fact(entry, "locked").as_deref(), Some("false"));
        // Sixteen bytes as recorded, padding included, beside the name.
        assert_eq!(
            fact(entry, "name-petscii").expect("recorded beside read").len(),
            16 * 3 - 1
        );
    }

    // The second file's name carries a byte PETSCII gives no reading —
    // the recorded bytes have it and the reading marks it, rather than
    // either of them being quietly dropped.
    assert!(entries[1].name.contains('\u{fffd}'), "{:?}", entries[1].name);
    assert_eq!(
        fact(&entries[1], "name-petscii").as_deref(),
        Some("45 41 22 9d a0 a0 a0 a0 a0 a0 a0 a0 a0 a0 a0 a0")
    );

    // Absence is an answer, and it is not the same as a failure to read
    // the directory (U3).
    assert!(space.stat("NOTHING").expect("the directory reads").is_none());
    assert_eq!(
        space
            .stat("demo1.pb")
            .expect("the directory reads")
            .map(|entry| entry.name),
        Some("DEMO1.PB".to_owned())
    );
}

#[test]
fn a_file_reads_by_name_and_its_size_is_what_its_chain_holds() {
    let mut space = presented()
        .sectors
        .partition()
        .filesystem_as("cbmdos")
        .expect("the disk bears it");
    let entries = space.entries("").expect("the directory chain reads");

    // Sixty-five blocks: sixty-four carrying their whole 254 bytes and
    // the last carrying what its own link field states it filled. The
    // directory records the block count and the walk establishes the
    // bytes, and both are reported.
    let program = entries
        .iter()
        .find(|entry| entry.name == "PCS.4000")
        .expect("the disk holds it");
    assert_eq!(fact(program, "size-blocks").as_deref(), Some("65"));
    assert_eq!(fact(program, "blocks-walked").as_deref(), Some("65"));
    assert_eq!(fact(program, "size-basis").as_deref(), Some("chain-walked"));
    assert_eq!(program.size_bytes, 64 * 254 + 130);

    let bytes = space.read_file("PCS.4000").expect("the chain reads");
    assert_eq!(bytes.len() as u64, program.size_bytes);
    // A Commodore program's first two bytes are the address it loads
    // at, low byte first — and this one is named for it. Four layers
    // down that is a flux transition, so the whole ladder is standing.
    assert_eq!(&bytes[..2], &[0x00, 0x40]);

    // The bounded form beside the whole-value one reaches the same
    // bytes (P27).
    let mut window = [0u8; 16];
    space
        .get_file("PCS.4000")
        .expect("the file is named")
        .read_at(1000, &mut window)
        .expect("a span within the file reads");
    assert_eq!(window, bytes[1000..1016]);
}

#[test]
fn a_file_whose_chain_reaches_an_unrecovered_block_is_qualified_rather_than_hidden() {
    let mut space = presented()
        .sectors
        .partition()
        .filesystem_as("cbmdos")
        .expect("the disk bears it");
    let entries = space.entries("").expect("the directory chain reads");

    // This capture never recovered track 16 sector 0, and one file's
    // chain runs through it. The listing still answers — what the
    // directory records is readable either way — and the entry says
    // which of the two its size came from and what stopped the walk.
    let broken = entries
        .iter()
        .find(|entry| entry.name == "PCS.A80A")
        .expect("the disk holds it");
    assert_eq!(
        fact(broken, "size-basis").as_deref(),
        Some("size-blocks-recorded")
    );
    assert_eq!(broken.size_bytes, 21 * 254);
    assert!(fact(broken, "blocks-walked").is_none());
    let refusal = fact(broken, "chain-refusal").expect("the entry says what stopped it");
    assert!(refusal.contains("track 16 sector 0"), "{refusal}");

    // And reading it is a refusal rather than the part that was
    // reachable: a file shortened to what came back would be worse than
    // no file at all.
    let error = space
        .read_file("PCS.A80A")
        .expect_err("the chain does not read");
    assert_eq!(error.category(), ErrorCategory::NotFound);
    assert_eq!(error.rule(), Some(SectorRule::NoSuchAddress.as_str()));

    // Its neighbours are untouched by it.
    assert_eq!(
        space.read_file("EA").expect("this one reads").len(),
        102
    );
}

#[test]
fn the_namespace_carries_the_file_verbs_and_claims_nothing_else() {
    // The sector layer carries no file verbs of its own. It composes a
    // partition and is asked what that partition composes; it does not
    // resolve a filesystem, and the verbs below live on the namespace
    // declared over the partition rather than on the layer beneath it.
    let mut space = presented()
        .sectors
        .partition()
        .filesystem_as("cbmdos")
        .expect("the disk bears it");

    // No addressable vantage: this namespace is presented over a layer
    // no volume composed, so there is no position to read by, and that
    // is a named refusal rather than a zero-filled answer.
    let mut buffer = [0u8; 4];
    let error = space
        .read_at(0, &mut buffer)
        .expect_err("there is no extent beneath this namespace");
    assert_eq!(error.rule(), Some("not-addressable"));

    // Read-only in this release, said by name at both the space and the
    // file.
    let error = space
        .write_file("EA", b"no")
        .expect_err("this release does not write CBM DOS");
    assert_eq!(error.rule(), Some("namespace-not-writable"));
    assert_eq!(error.category(), ErrorCategory::ReadOnly);
    let error = space
        .get_file("EA")
        .expect("the file is named")
        .write_at(0, b"no")
        .expect_err("nor through the file");
    assert_eq!(error.rule(), Some("namespace-not-writable"));

    // A path below the root holds no names: CBM DOS is one flat
    // directory, and saying so is not the same as answering empty.
    let error = space
        .entries("SOMEWHERE")
        .expect_err("the directory is flat");
    assert_eq!(error.category(), ErrorCategory::NotDirectory);
}

#[test]
fn the_grammar_and_the_whole_ladder_beneath_it_are_stated() {
    let report = presented().sectors.inspect();

    let says = |fragment: &str| {
        assert!(
            report.evidence.iter().any(|line| line.contains(fragment)),
            "{fragment:?} is not in {:?}",
            report.evidence
        );
    };
    // The grammar every rule came from, and the pairing rule stated as
    // the grammatical thing it is.
    says("CBM DOS sector record");
    says("carries next after its header");
    // The seam itself: this layer says it is where the silence ends,
    // and the layer below is still saying it assigns nothing.
    says("this is where the bytestream's silence ends");
    says("the bytestream beneath it: no byte here is a header");
    // And the reduction that produced the disk is readable from up here.
    says("served projection of a remanence image");
    says("the medium beneath it: recordings: measured");

    // The strict policy stops on the first block whose stated checksum
    // its own bytes do not compute, and names where it is.
    let (category, rule, message) = presented()
        .failed_checksum
        .as_ref()
        .expect("a real recording holds a block that fails its own checksum");
    assert_eq!(*category, ErrorCategory::InvalidImage);
    assert_eq!(rule.as_deref(), Some(SectorRule::BlockFailedChecksum.as_str()));
    assert!(message.contains("half-track"), "{message}");
    assert!(message.contains("the policy refuses"), "{message}");
}
