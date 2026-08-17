// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The records an FM or MFM recording states for itself (F78): the
//! address it claims for each sector, the data field that follows, and
//! both checksums stated beside computed.
//!
//! **The recording is read, never repaired.** A field whose stored CRC
//! disagrees with the one its own bytes compute is reported with both
//! numbers, not corrected and not dropped; an id field with no data
//! field after it is a header that holds no data and says so; a data
//! field opened by a deleted-data mark is what the recording says and is
//! carried as such. Nothing here decides on a caller's behalf whether a
//! sector counts.
//!
//! **Pairing is by order, not by distance.** A recording writes its data
//! field after the id field it belongs to, so a sector's data is the
//! next data field before the next id field. Nothing measures a gap and
//! nothing tunes a window — a header whose next field is another header
//! simply has no data.
//!
//! What this module does *not* do is decide which locations exist, at
//! what rate they were clocked, or which family declared any of it. It
//! reads one location's cells and reports what it found.

use crate::checksum::Crc16Ccitt;
use crate::flux::ibm::encoding::{
    self, Cells, Encoding, FM_DATA_CELLS, FM_DELETED_CELLS, FM_ID_CELLS, MFM_A1, MFM_A1_CELLS,
    decode_byte,
};

/// The id-address-mark byte, which follows the sync marks in MFM and is
/// itself the mark in FM.
const IDAM: u8 = 0xfe;
/// The data-address-mark byte.
const DAM: u8 = 0xfb;
/// The deleted-data-address-mark byte.
const DELETED_DAM: u8 = 0xf8;

/// How many `A1` marks introduce an MFM field.
const MFM_SYNC_MARKS: usize = 3;

/// The address a sector states for itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SectorAddress {
    pub(crate) cylinder: u8,
    pub(crate) head: u8,
    pub(crate) sector: u8,
    /// The size code as recorded. The byte count is `128 << code`, and
    /// the code is kept because it is what the recording states.
    pub(crate) size_code: u8,
}

impl SectorAddress {
    /// The bytes this address's size code declares.
    pub(crate) fn bytes(self) -> usize {
        128usize << self.size_code.min(7)
    }
}

/// A checksum as the recording states it, beside the one its bytes make.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Checksums {
    pub(crate) stated: u16,
    pub(crate) computed: u16,
}

impl Checksums {
    pub(crate) fn agree(self) -> bool {
        self.stated == self.computed
    }
}

/// What one recognized record holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SectorRecord {
    /// Where the id field's mark began, in cells.
    pub(crate) at_cell: usize,
    pub(crate) address: SectorAddress,
    pub(crate) header: Checksums,
    /// The data field that followed, where one did.
    pub(crate) data: Option<DataField>,
}

/// The data field belonging to one id field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DataField {
    pub(crate) at_cell: usize,
    /// Whether the mark that opened it was the deleted-data one. It is
    /// a fact the recording states, carried rather than judged.
    pub(crate) deleted: bool,
    pub(crate) bytes: Vec<u8>,
    pub(crate) checksums: Checksums,
}

/// A field's mark, once located.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mark {
    Id,
    Data { deleted: bool },
}

/// One located mark: what it was, where its *payload* begins, and the
/// bytes the checksum is seeded with.
#[derive(Debug, Clone)]
struct Located {
    mark: Mark,
    at_cell: usize,
    payload_cell: usize,
    covered: Vec<u8>,
}

/// Finds every address mark in a location's cells, in the order recorded.
fn locate(bits: &[bool], encoding: Encoding) -> Vec<Located> {
    let mut found: Vec<Located> = Vec::new();
    match encoding {
        Encoding::Mfm => {
            // Three A1 marks then the byte saying which field this is.
            for start in encoding::find_marks(bits, MFM_A1_CELLS) {
                let run_end = start + MFM_SYNC_MARKS * 16;
                // The three marks must actually be consecutive; a lone
                // A1 is not a field introduction.
                let complete = (0..MFM_SYNC_MARKS).all(|index| {
                    encoding::cells_at(bits, start + index * 16) == Some(MFM_A1_CELLS)
                });
                if !complete {
                    continue;
                }
                // Only the first of a run introduces a field.
                if found
                    .last()
                    .is_some_and(|last| start < last.payload_cell && start >= last.at_cell)
                {
                    continue;
                }
                let Some(kind) = encoding::cells_at(bits, run_end).map(decode_byte) else {
                    continue;
                };
                let mark = match kind {
                    IDAM => Mark::Id,
                    DAM => Mark::Data { deleted: false },
                    DELETED_DAM => Mark::Data { deleted: true },
                    _ => continue,
                };
                found.push(Located {
                    mark,
                    at_cell: start,
                    payload_cell: run_end + 16,
                    covered: vec![MFM_A1, MFM_A1, MFM_A1, kind],
                });
            }
        }
        Encoding::Fm => {
            // FM's marks are single bytes whose own clocks are the
            // violation, so each is looked for by its exact cells.
            for (cells, mark, byte) in [
                (FM_ID_CELLS, Mark::Id, IDAM),
                (FM_DATA_CELLS, Mark::Data { deleted: false }, DAM),
                (FM_DELETED_CELLS, Mark::Data { deleted: true }, DELETED_DAM),
            ] {
                for start in encoding::find_marks(bits, cells as Cells) {
                    found.push(Located {
                        mark,
                        at_cell: start,
                        payload_cell: start + 16,
                        covered: vec![byte],
                    });
                }
            }
            found.sort_by_key(|located| located.at_cell);
        }
    }
    found
}

/// Reads `count` bytes of payload starting at a cell position.
fn read_bytes(bits: &[bool], at_cell: usize, count: usize) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(count);
    for index in 0..count {
        out.push(decode_byte(encoding::cells_at(bits, at_cell + index * 16)?));
    }
    Some(out)
}

/// The checksum over a field: the marks it was introduced by, then its
/// own bytes.
fn checksum(covered: &[u8], payload: &[u8]) -> u16 {
    let mut crc = Crc16Ccitt::new();
    crc.update(covered);
    crc.update(payload);
    crc.finish()
}

/// Recognizes every record one location's cells state.
///
/// The cells are one whole revolution, so a field may be truncated at
/// the end; a truncated field is left out rather than reported with the
/// bytes that happen to follow it.
pub(crate) fn recognize(bits: &[bool], encoding: Encoding) -> Vec<SectorRecord> {
    let located = locate(bits, encoding);
    let mut records: Vec<SectorRecord> = Vec::new();

    for (index, field) in located.iter().enumerate() {
        let Mark::Id = field.mark else { continue };
        // Four address bytes then the stored checksum.
        let Some(payload) = read_bytes(bits, field.payload_cell, 4) else {
            continue;
        };
        let Some(stored) = read_bytes(bits, field.payload_cell + 4 * 16, 2) else {
            continue;
        };
        let address = SectorAddress {
            cylinder: payload[0],
            head: payload[1],
            sector: payload[2],
            size_code: payload[3],
        };
        let header = Checksums {
            stated: u16::from_be_bytes([stored[0], stored[1]]),
            computed: checksum(&field.covered, &payload),
        };

        // The data field is the next field, and only if it is a data
        // one: an id field followed by another id field holds no data.
        let data = located.get(index + 1).and_then(|next| match next.mark {
            Mark::Data { deleted } => {
                let bytes = read_bytes(bits, next.payload_cell, address.bytes())?;
                let stored = read_bytes(bits, next.payload_cell + address.bytes() * 16, 2)?;
                Some(DataField {
                    at_cell: next.at_cell,
                    deleted,
                    checksums: Checksums {
                        stated: u16::from_be_bytes([stored[0], stored[1]]),
                        computed: checksum(&next.covered, &bytes),
                    },
                    bytes,
                })
            }
            Mark::Id => None,
        });

        records.push(SectorRecord {
            at_cell: field.at_cell,
            address,
            header,
            data,
        });
    }
    records
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flux::ibm::encoding::{encode, encode_mark};

    /// Writes one sector the way a recording does: gap, sync, id field,
    /// gap, sync, data field. The bytes are what an IBM track holds.
    struct TrackWriter {
        bits: Vec<bool>,
        last: bool,
        encoding: Encoding,
    }

    impl TrackWriter {
        fn new(encoding: Encoding) -> Self {
            Self {
                bits: Vec::new(),
                last: false,
                encoding,
            }
        }

        fn bytes(&mut self, data: &[u8]) -> &mut Self {
            let (bits, last) = encode(self.encoding, data, self.last);
            self.bits.extend(bits);
            self.last = last;
            self
        }

        /// The marks that introduce a field, laid down as the violations
        /// they are.
        fn marks(&mut self, kind: u8) -> &mut Self {
            match self.encoding {
                Encoding::Mfm => {
                    for _ in 0..MFM_SYNC_MARKS {
                        self.bits.extend(encode_mark(MFM_A1_CELLS));
                    }
                    self.last = MFM_A1 & 1 == 1;
                    self.bytes(&[kind]);
                }
                Encoding::Fm => {
                    let cells = match kind {
                        IDAM => FM_ID_CELLS,
                        DAM => FM_DATA_CELLS,
                        DELETED_DAM => FM_DELETED_CELLS,
                        other => panic!("no FM mark for {other:#04x}"),
                    };
                    self.bits.extend(encode_mark(cells));
                    self.last = kind & 1 == 1;
                }
            }
            self
        }

        fn sector(&mut self, address: SectorAddress, data: &[u8], deleted: bool) -> &mut Self {
            let covered_kind = if deleted { DELETED_DAM } else { DAM };
            let id = [
                address.cylinder,
                address.head,
                address.sector,
                address.size_code,
            ];

            self.bytes(&[0x4e; 12]);
            self.marks(IDAM);
            self.bytes(&id);
            let mut crc = Crc16Ccitt::new();
            if self.encoding == Encoding::Mfm {
                crc.update(&[MFM_A1, MFM_A1, MFM_A1]);
            }
            crc.update(&[IDAM]);
            crc.update(&id);
            self.bytes(&crc.finish().to_be_bytes());

            self.bytes(&[0x4e; 12]);
            self.marks(covered_kind);
            self.bytes(data);
            let mut crc = Crc16Ccitt::new();
            if self.encoding == Encoding::Mfm {
                crc.update(&[MFM_A1, MFM_A1, MFM_A1]);
            }
            crc.update(&[covered_kind]);
            crc.update(data);
            self.bytes(&crc.finish().to_be_bytes());
            self
        }
    }

    fn address(sector: u8) -> SectorAddress {
        SectorAddress {
            cylinder: 0,
            head: 0,
            sector,
            size_code: 0,
        }
    }

    #[test]
    fn a_written_track_reads_back_as_the_sectors_it_states() {
        for encoding in [Encoding::Fm, Encoding::Mfm] {
            let mut track = TrackWriter::new(encoding);
            let first: Vec<u8> = (0..128u8).collect();
            let second: Vec<u8> = (0..128u8).map(|at| at ^ 0x5a).collect();
            track.bytes(&[0x4e; 32]);
            track.sector(address(1), &first, false);
            track.sector(address(2), &second, false);

            let records = recognize(&track.bits, encoding);
            assert_eq!(records.len(), 2, "{encoding:?}");
            assert_eq!(records[0].address, address(1));
            assert_eq!(records[1].address, address(2));
            assert!(records[0].header.agree(), "{encoding:?} header 1");
            assert!(records[1].header.agree(), "{encoding:?} header 2");

            let data = records[0].data.as_ref().expect("a data field followed");
            assert_eq!(data.bytes, first, "{encoding:?}");
            assert!(data.checksums.agree());
            assert!(!data.deleted);
            assert_eq!(
                records[1].data.as_ref().expect("followed").bytes,
                second,
                "{encoding:?}"
            );
        }
    }

    #[test]
    fn a_deleted_data_mark_is_carried_and_the_bytes_are_still_served() {
        for encoding in [Encoding::Fm, Encoding::Mfm] {
            let mut track = TrackWriter::new(encoding);
            let payload: Vec<u8> = (0..128u8).map(|at| at.wrapping_add(3)).collect();
            track.bytes(&[0x4e; 32]);
            track.sector(address(7), &payload, true);

            let records = recognize(&track.bits, encoding);
            assert_eq!(records.len(), 1, "{encoding:?}");
            let data = records[0].data.as_ref().expect("a data field followed");
            assert!(data.deleted, "{encoding:?}: the recording said deleted");
            assert_eq!(data.bytes, payload, "and the bytes are what it holds");
            assert!(data.checksums.agree());
        }
    }

    #[test]
    fn a_corrupted_data_field_reports_both_checksums_and_is_not_dropped() {
        let encoding = Encoding::Mfm;
        let written = |payload: &[u8]| {
            let mut track = TrackWriter::new(encoding);
            track.bytes(&[0x4e; 32]);
            track.sector(address(1), payload, false);
            track
        };
        let payload: Vec<u8> = (0..128u8).collect();

        // Cells alternate clock, data, clock, data from the start of the
        // track, so an odd position is a data cell. Flipping one changes
        // the byte the field decodes to, and its stored checksum — which
        // the recording wrote before the damage — no longer matches.
        let mut damaged = written(&payload);
        let flip = damaged.bits.len() - 599;
        assert_eq!(flip % 2, 1, "an odd position is a data cell");
        damaged.bits[flip] = !damaged.bits[flip];

        let records = recognize(&damaged.bits, encoding);
        assert_eq!(records.len(), 1);
        let data = records[0].data.as_ref().expect("the field is still there");
        assert!(
            !data.checksums.agree(),
            "the recording's own number and the bytes' disagree"
        );
        assert_ne!(
            data.bytes, payload,
            "and the bytes are not what was written"
        );
        // The header is untouched, so the sector is still addressed: one
        // damaged field spoils itself and nothing else.
        assert!(records[0].header.agree());

        // The other half of the same fact: a *clock* cell carries no data,
        // so flipping one leaves the decoded field and its checksum
        // exactly as they were. Whether the recording was lawful there is
        // a different question from what it says, and this layer answers
        // the second.
        let mut reclocked = written(&payload);
        let clock = reclocked.bits.len() - 600;
        assert_eq!(clock % 2, 0, "an even position is a clock cell");
        reclocked.bits[clock] = !reclocked.bits[clock];

        let records = recognize(&reclocked.bits, encoding);
        let data = records[0].data.as_ref().expect("the field is still there");
        assert_eq!(data.bytes, payload);
        assert!(data.checksums.agree());
    }

    #[test]
    fn a_header_with_no_data_field_after_it_holds_no_data() {
        let encoding = Encoding::Mfm;
        let mut track = TrackWriter::new(encoding);
        // Two id fields in a row: the first is a header whose data was
        // never written.
        track.bytes(&[0x4e; 32]);
        track.marks(IDAM);
        let id = [0, 0, 5, 0];
        track.bytes(&id);
        let mut crc = Crc16Ccitt::new();
        crc.update(&[MFM_A1, MFM_A1, MFM_A1, IDAM]);
        crc.update(&id);
        track.bytes(&crc.finish().to_be_bytes());
        track.sector(address(6), &(0..128u8).collect::<Vec<u8>>(), false);

        let records = recognize(&track.bits, encoding);
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].address.sector, 5);
        assert!(
            records[0].data.is_none(),
            "nothing follows it but another header"
        );
        assert_eq!(records[1].address.sector, 6);
        assert!(records[1].data.is_some());
    }

    #[test]
    fn the_size_code_the_recording_states_is_what_the_data_field_holds() {
        let encoding = Encoding::Mfm;
        for (code, bytes) in [(0u8, 128usize), (1, 256), (2, 512)] {
            let mut track = TrackWriter::new(encoding);
            let payload: Vec<u8> = (0..bytes).map(|at| (at % 251) as u8).collect();
            track.bytes(&[0x4e; 32]);
            track.sector(
                SectorAddress {
                    cylinder: 3,
                    head: 1,
                    sector: 9,
                    size_code: code,
                },
                &payload,
                false,
            );

            let records = recognize(&track.bits, encoding);
            assert_eq!(records.len(), 1, "size code {code}");
            assert_eq!(records[0].address.size_code, code);
            let data = records[0].data.as_ref().expect("followed");
            assert_eq!(data.bytes.len(), bytes);
            assert_eq!(data.bytes, payload);
            assert!(data.checksums.agree());
        }
    }

    #[test]
    fn a_field_the_revolution_cuts_short_is_left_out_rather_than_padded() {
        let encoding = Encoding::Mfm;
        let mut track = TrackWriter::new(encoding);
        track.bytes(&[0x4e; 32]);
        track.sector(address(1), &(0..128u8).collect::<Vec<u8>>(), false);
        // Cut the track inside the data field.
        track.bits.truncate(track.bits.len() - 400);

        let records = recognize(&track.bits, encoding);
        assert_eq!(records.len(), 1, "the header is whole and still reads");
        assert!(
            records[0].data.is_none(),
            "the data field does not fit and is not reported with whatever followed"
        );
    }

    #[test]
    fn data_that_reads_like_a_mark_does_not_frame_a_field() {
        // The point of the clock violation. A sector whose payload is
        // full of A1 bytes must not be read as containing address marks,
        // because lawfully-encoded A1 is a different cell pattern.
        let encoding = Encoding::Mfm;
        let mut track = TrackWriter::new(encoding);
        let payload = vec![MFM_A1; 128];
        track.bytes(&[0x4e; 32]);
        track.sector(address(1), &payload, false);

        let records = recognize(&track.bits, encoding);
        assert_eq!(records.len(), 1, "one sector, not one per payload byte");
        let data = records[0].data.as_ref().expect("followed");
        assert_eq!(data.bytes, payload);
        assert!(data.checksums.agree());
    }
}
