// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! An HxC Floppy Emulator MFM container, read through the ladder (F77).
//!
//! The containers here are written by the test rather than fetched,
//! because what is being proved is the reader's own arithmetic: that the
//! cells the container states are the cells the channel clocks, and that
//! every declaration the file makes is checked against the family rather
//! than taken on trust. A third-party artifact would prove something
//! else worth proving — that this reader agrees with the tool that
//! writes them — and that check is still owed.

use std::path::PathBuf;

use remanence::{FloppyDrive, Format, Session};

/// The header is the signature and six declarations.
const HEADER_BYTES: usize = 19;
/// Each track states its number, side, size and where its cells are.
const ENTRY_BYTES: usize = 11;

/// What a container of the H-17-4's family declares.
struct Declared {
    tracks: u16,
    sides: u8,
    rpm: u16,
    bitrate_kbps: u16,
    interface_type: u8,
}

impl Declared {
    fn h17_4() -> Self {
        Self {
            tracks: 2,
            sides: 2,
            rpm: 300,
            // The container states the data rate, which is half the
            // family's 500 kHz cell rate.
            bitrate_kbps: 250,
            interface_type: 1,
        }
    }
}

fn scratch(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!("remanence-hxc-{tag}-{}.mfm", std::process::id()))
}

fn placed(tag: &str, bytes: &[u8]) -> PathBuf {
    let path = scratch(tag);
    let _ = std::fs::remove_file(&path);
    std::fs::write(&path, bytes).expect("the scratch container is written");
    path
}

// --------------------------------------------------------------- MFM

/// The MFM cell rules, written out here rather than reached for: the
/// test must be able to say what a correct container holds without
/// asking the code under test.
fn encode_mfm(data: &[u8], mut last: bool) -> (Vec<bool>, bool) {
    let mut cells = Vec::with_capacity(data.len() * 16);
    for byte in data {
        for shift in (0..8).rev() {
            let bit = byte >> shift & 1 == 1;
            // A clock cell only where neither neighbour is a one.
            cells.push(!last && !bit);
            cells.push(bit);
            last = bit;
        }
    }
    (cells, last)
}

/// The A1 address mark, as the deliberate clock violation it is.
fn encode_a1() -> Vec<bool> {
    // 0x4489: the A1 byte with the clock between bits 4 and 5 missing.
    (0..16)
        .rev()
        .map(|shift| 0x4489u16 >> shift & 1 == 1)
        .collect()
}

fn crc16(covered: &[u8]) -> u16 {
    let mut crc = 0xffffu16;
    for byte in covered {
        crc ^= u16::from(*byte) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 {
                crc << 1 ^ 0x1021
            } else {
                crc << 1
            };
        }
    }
    crc
}

/// One track of MFM cells holding `sectors` records of 256 bytes.
fn track_cells(cylinder: u8, head: u8, sectors: u8) -> Vec<bool> {
    let mut cells = Vec::new();
    let mut last = false;
    let write = |bytes: &[u8], cells: &mut Vec<bool>, last: &mut bool| {
        let (encoded, ended) = encode_mfm(bytes, *last);
        cells.extend(encoded);
        *last = ended;
    };

    for sector in 1..=sectors {
        for (mark, field) in [
            (0xfeu8, vec![cylinder, head, sector, 1u8]),
            (0xfbu8, vec![sector; 256]),
        ] {
            write(&[0x4e; 12], &mut cells, &mut last);
            for _ in 0..3 {
                cells.extend(encode_a1());
            }
            last = 0xa1 & 1 == 1;
            write(&[mark], &mut cells, &mut last);
            write(&field, &mut cells, &mut last);
            let mut covered = vec![0xa1, 0xa1, 0xa1, mark];
            covered.extend_from_slice(&field);
            write(&crc16(&covered).to_be_bytes(), &mut cells, &mut last);
        }
    }
    cells
}

// --------------------------------------------------- the container

fn pack(cells: &[bool]) -> Vec<u8> {
    // The container's own bit order: most significant bit first, which
    // is how HxC writes them and how the encoding reads them.
    let mut packed = vec![0u8; cells.len().div_ceil(8)];
    for (at, cell) in cells.iter().enumerate() {
        if *cell {
            packed[at / 8] |= 1 << (7 - at % 8);
        }
    }
    packed
}

fn container(declared: &Declared, tracks: &[(u16, u8, Vec<bool>)]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"HXCMFM\0");
    out.extend_from_slice(&declared.tracks.to_le_bytes());
    out.push(declared.sides);
    out.extend_from_slice(&declared.rpm.to_le_bytes());
    out.extend_from_slice(&declared.bitrate_kbps.to_le_bytes());
    out.push(declared.interface_type);
    out.extend_from_slice(&(HEADER_BYTES as u32).to_le_bytes());
    assert_eq!(out.len(), HEADER_BYTES);

    let list_at = out.len();
    out.resize(list_at + tracks.len() * ENTRY_BYTES, 0);

    let mut at = out.len();
    for (index, (track, side, cells)) in tracks.iter().enumerate() {
        let packed = pack(cells);
        let entry = list_at + index * ENTRY_BYTES;
        out[entry..entry + 2].copy_from_slice(&track.to_le_bytes());
        out[entry + 2] = *side;
        out[entry + 3..entry + 7].copy_from_slice(&(packed.len() as u32).to_le_bytes());
        out[entry + 7..entry + 11].copy_from_slice(&(at as u32).to_le_bytes());
        out.extend_from_slice(&packed);
        at += packed.len();
    }
    out
}

fn whole_disk(declared: &Declared) -> Vec<u8> {
    let mut tracks = Vec::new();
    for cylinder in 0..declared.tracks {
        for head in 0..declared.sides {
            tracks.push((cylinder, head, track_cells(cylinder as u8, head, 4)));
        }
    }
    container(declared, &tracks)
}

fn load(tag: &str, bytes: &[u8], device: FloppyDrive) -> (Session, remanence::Result<()>, PathBuf) {
    let path = placed(tag, bytes);
    let mut session = Session::new();
    let file = std::fs::File::open(&path).expect("the container opens");
    let outcome = session
        .load_media(file, Format::HxcMfm { device })
        .map(|_| ());
    (session, outcome, path)
}

// ------------------------------------------------------------ tests

#[test]
fn a_container_is_read_and_its_cells_reach_the_records_it_was_written_from() {
    let declared = Declared::h17_4();
    let path = placed("whole", &whole_disk(&declared));
    let mut session = Session::new();
    let file = std::fs::File::open(&path).expect("the container opens");
    let id = session
        .load_media(
            file,
            Format::HxcMfm {
                device: FloppyDrive::HeathH37Dd,
            },
        )
        .expect("the declared container loads")
        .id();

    let medium = session.medium_mut(id).expect("the medium is pooled");

    // The container's own declarations travel as evidence, and so does
    // the fact that its flux layer is this reader's restatement.
    let assurance = medium.assurance();
    let evidence = assurance.evidence.join("\n");
    assert!(evidence.contains("250 kbit/s"), "{evidence}");
    assert!(evidence.contains("300 RPM"), "{evidence}");
    assert!(
        evidence.contains("no weak region"),
        "the absences are stated rather than filled in: {evidence}"
    );
    assert!(
        evidence.contains("declared synthetic"),
        "the flux beneath the cells is not presented as recovered timing: {evidence}"
    );

    // And the whole point of the tier: the cells the container states
    // are the cells the channel clocks, so the records come back.
    // Reached the way a caller reaches it: up the ladder from the
    // bytestream, then narrowed to the family that made these records.
    let sectors = medium
        .bytestream()
        .expect("the codec resolves the container's cells")
        .recognize_sectors(1 << 20)
        .expect("the grammar reads the container's own records")
        .into_ibm()
        .expect("an IBM recording answers the IBM reading");
    assert_eq!(
        sectors.claim_count(),
        16,
        "four records on each of four tracks"
    );
    assert!(
        sectors
            .inspect()
            .claims
            .iter()
            .all(|claim| claim.readable()),
        "every record comes back with both checks agreeing"
    );
    assert_eq!(
        sectors.read_sector(1, 1, 3).expect("one record"),
        vec![3u8; 256]
    );

    session.release_media(id).expect("the medium releases");
    std::fs::remove_file(&path).ok();
}

#[test]
fn a_rate_the_family_does_not_record_is_refused_showing_both_numbers() {
    // 500 kbit/s is the PC's high-density rate, and a container stating
    // it is not a recording of a double-density Heath family.
    let mut declared = Declared::h17_4();
    declared.bitrate_kbps = 500;
    let (_session, outcome, path) = load(
        "wrong-rate",
        &whole_disk(&declared),
        FloppyDrive::HeathH37Dd,
    );

    let error = outcome.expect_err("the family records at 250 kbit/s");
    let said = error.to_string();
    assert!(said.contains("states 500 kbit/s"), "{said}");
    assert!(said.contains("records 250 kbit/s"), "{said}");
    assert!(said.contains("a mismatch is refused"), "{said}");
    std::fs::remove_file(&path).ok();
}

#[test]
fn a_measured_rate_within_the_band_is_the_familys_and_the_figure_is_declared() {
    // HxC's writer states the rate it measured off the track it
    // converted, so a 250 kbit/s recording arrives as 251 or 249.
    let mut declared = Declared::h17_4();
    declared.bitrate_kbps = 251;
    let (mut session, outcome, path) = load(
        "measured-rate",
        &whole_disk(&declared),
        FloppyDrive::HeathH37Dd,
    );
    outcome.expect("a figure one part in two hundred and fifty off is a measurement");

    let id = *session.media().first().expect("pooled");
    let medium = session.medium_mut(id).expect("pooled");
    let evidence = medium.assurance().evidence.join(
        "
",
    );
    assert!(
        evidence.contains("states 251 kbit/s, a measured figure"),
        "{evidence}"
    );
    assert!(evidence.contains("family's 250 kbit/s"), "{evidence}");

    // And the cells were laid at the family's rate, not the stated one:
    // the records still come back.
    let sectors = medium
        .bytestream()
        .expect("the codec resolves the container's cells")
        .recognize_sectors(1 << 20)
        .expect("the grammar reads the container's own records")
        .into_ibm()
        .expect("an IBM recording answers the IBM reading");
    assert_eq!(sectors.claim_count(), 16);
    std::fs::remove_file(&path).ok();
}

#[test]
fn an_unstated_rotation_takes_the_familys_and_declares_the_absence() {
    // HxC's writer puts zero in the RPM field unconditionally, so zero
    // is the writer declining to state one rather than a rotation of
    // zero.
    let mut declared = Declared::h17_4();
    declared.rpm = 0;
    let (mut session, outcome, path) =
        load("no-rpm", &whole_disk(&declared), FloppyDrive::HeathH37Dd);
    outcome.expect("an unstated rotation is the family's");

    let id = *session.media().first().expect("pooled");
    let medium = session.medium_mut(id).expect("pooled");
    let evidence = medium.assurance().evidence.join(
        "
",
    );
    assert!(evidence.contains("states no RPM"), "{evidence}");
    assert!(evidence.contains("family's 300 RPM is taken"), "{evidence}");
    std::fs::remove_file(&path).ok();
}

#[test]
fn cells_past_one_revolution_are_left_out_and_counted() {
    // A real container holds a little more than one revolution, the
    // writer having read past the index it started from. The family's
    // circle holds 100,000 cells; this track states 101,000, and the
    // thousand past the circle are an angle on nothing.
    let declared = Declared::h17_4();
    let mut long = track_cells(0, 0, 4);
    let (gap, _) = encode_mfm(&[0x4e; 1], false);
    while long.len() < 101_000 {
        long.extend(&gap);
    }
    long.truncate(101_000);
    let tracks = vec![
        (0u16, 0u8, long),
        (0, 1, track_cells(0, 1, 4)),
        (1, 0, track_cells(1, 0, 4)),
        (1, 1, track_cells(1, 1, 4)),
    ];
    let (mut session, outcome, path) = load(
        "long-track",
        &container(&declared, &tracks),
        FloppyDrive::HeathH37Dd,
    );
    outcome.expect("a track longer than the circle loads");

    let id = *session.media().first().expect("pooled");
    let medium = session.medium_mut(id).expect("pooled");
    let evidence = medium.assurance().evidence.join(
        "
",
    );
    assert!(
        evidence.contains("1 track(s) state more cells than one revolution"),
        "{evidence}"
    );
    assert!(
        evidence.contains("'hxc-mfm.track-longer-than-the-circle' × 1000"),
        "{evidence}"
    );

    // The records on the circle still read — all four tracks' worth.
    let sectors = medium
        .bytestream()
        .expect("the codec resolves the container's cells")
        .recognize_sectors(1 << 20)
        .expect("the grammar reads the container's own records")
        .into_ibm()
        .expect("an IBM recording answers the IBM reading");
    assert_eq!(sectors.claim_count(), 16);
    std::fs::remove_file(&path).ok();
}

#[test]
fn a_rotation_the_family_does_not_turn_at_is_refused() {
    let mut declared = Declared::h17_4();
    declared.rpm = 360;
    let (_session, outcome, path) =
        load("wrong-rpm", &whole_disk(&declared), FloppyDrive::HeathH37Dd);

    let error = outcome.expect_err("the family turns at 300 RPM");
    let said = error.to_string();
    assert!(said.contains("360"), "{said}");
    assert!(said.contains("300"), "{said}");
    std::fs::remove_file(&path).ok();
}

#[test]
fn a_recording_of_another_familys_surfaces_is_refused_by_name() {
    // The H-17-1 records one surface. A two-sided container is not a
    // recording of that family, and reading it as one would put every
    // second track's cells on a side the mechanism does not have.
    let (_session, outcome, path) = load(
        "wrong-sides",
        &whole_disk(&Declared::h17_4()),
        FloppyDrive::HeathH37,
    );

    let error = outcome.expect_err("one surface is not two");
    let said = error.to_string();
    assert!(said.contains("2 side(s)"), "{said}");
    assert!(said.contains("records 1"), "{said}");
    std::fs::remove_file(&path).ok();
}

#[test]
fn a_file_that_is_not_a_container_is_refused_before_anything_is_read() {
    let mut bytes = whole_disk(&Declared::h17_4());
    bytes[0] = b'X';
    let (_session, outcome, path) = load("not-one", &bytes, FloppyDrive::HeathH37Dd);

    let error = outcome.expect_err("the signature is not the format's");
    assert!(error.to_string().contains("HxC MFM signature"), "{error}");
    std::fs::remove_file(&path).ok();
}

#[test]
fn a_track_the_container_holds_no_cells_for_is_absent_rather_than_empty() {
    let declared = Declared::h17_4();
    let mut tracks = Vec::new();
    for cylinder in 0..declared.tracks {
        for head in 0..declared.sides {
            let cells = if cylinder == 1 && head == 1 {
                Vec::new()
            } else {
                track_cells(cylinder as u8, head, 4)
            };
            tracks.push((cylinder, head, cells));
        }
    }
    let path = placed("hole", &container(&declared, &tracks));
    let mut session = Session::new();
    let file = std::fs::File::open(&path).expect("the container opens");
    let id = session
        .load_media(
            file,
            Format::HxcMfm {
                device: FloppyDrive::HeathH37Dd,
            },
        )
        .expect("a container with an unformatted track still loads")
        .id();

    let medium = session.medium_mut(id).expect("the medium is pooled");
    let evidence = medium.assurance().evidence.join("\n");
    assert!(
        evidence.contains("track-holds-no-cells"),
        "the absent track is declared rather than silent: {evidence}"
    );

    // The three tracks that do hold cells still read whole.
    let sectors = medium
        .bytestream()
        .expect("the codec resolves the cells that are there")
        .recognize_sectors(1 << 20)
        .expect("the rest of the disk reads")
        .into_ibm()
        .expect("an IBM recording answers the IBM reading");
    assert_eq!(sectors.claim_count(), 12);

    session.release_media(id).expect("the medium releases");
    std::fs::remove_file(&path).ok();
}

#[test]
fn a_track_stated_past_the_end_of_the_file_is_refused() {
    let mut bytes = whole_disk(&Declared::h17_4());
    let past = bytes.len() as u32 + 4096;
    bytes[HEADER_BYTES + 7..HEADER_BYTES + 11].copy_from_slice(&past.to_le_bytes());
    let (_session, outcome, path) = load("past-end", &bytes, FloppyDrive::HeathH37Dd);

    let error = outcome.expect_err("the track is not in the file");
    assert!(error.to_string().contains("holds"), "{error}");
    std::fs::remove_file(&path).ok();
}
