// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! FM and MFM cell rules, and the address marks that frame them (F78).
//!
//! **This is the part of the channel that is arithmetic rather than
//! policy**, so it is a module of plain functions over bit slices, with
//! no medium, no profile and no session under it. What it knows is how
//! the two encodings lay a byte down as cells and how a recording says
//! "a field starts here"; everything about *which* recording, at what
//! rate, from which drive, belongs to the profile above.
//!
//! **A cell stream is clock and data interleaved.** Both encodings write
//! two cells per data bit — a clock cell then a data cell — and differ
//! only in when the clock cell is written:
//!
//! - **FM** writes a clock transition before every data bit, always.
//! - **MFM** writes one only between two zero bits, which is what makes
//!   it twice as dense for the same cell rate: a run of ones carries no
//!   clock at all, and the rule that decides is the *previous* data bit.
//!
//! **Framing is a deliberate violation of that rule, and it has to be.**
//! Neither encoding has an illegal byte — every value is writable — so a
//! recording cannot mark a field boundary with a reserved value the way
//! a group code can. What it does instead is write a byte whose *clock*
//! pattern the rule forbids: MFM's `A1` mark drops the clock between
//! bits 4 and 3, and FM's marks write only some of their clocks. Nothing
//! but the mark produces those cell patterns, which is exactly why they
//! can begin a field.
//!
//! That is the fact this module exists to preserve. A decoder that threw
//! away clock cells and kept the data would decode the marks into
//! ordinary bytes and lose the only thing that distinguishes them from
//! data that happens to read `A1`.

/// One cell pattern, as the sixteen cells of one encoded byte.
///
/// Cells are ordered as recorded: clock, data, clock, data, and so on
/// from the most significant data bit down.
pub(crate) type Cells = u16;

/// The MFM address-mark byte every IBM System 34 field is introduced by,
/// three of them in a row.
pub(crate) const MFM_A1: u8 = 0xa1;

/// `A1` as it is actually recorded: the ordinary MFM cells for `A1` with
/// one clock cell suppressed.
///
/// `A1` is `1010 0001`. Its lawful clock pattern would put a clock
/// between the two adjacent zero bits at positions 4 and 3; the mark
/// omits it. No lawful encoding of any byte produces this, which is what
/// makes it a landmark rather than a value.
pub(crate) const MFM_A1_CELLS: Cells = 0x4489;

/// The FM index address mark, `FC` with the `D7` clock.
pub(crate) const FM_INDEX_CELLS: Cells = 0xf77a;

/// The FM id address mark, `FE` with the `C7` clock.
pub(crate) const FM_ID_CELLS: Cells = 0xf57e;

/// The FM data address mark, `FB` with the `C7` clock.
pub(crate) const FM_DATA_CELLS: Cells = 0xf56f;

/// The FM deleted-data address mark, `F8` with the `C7` clock.
pub(crate) const FM_DELETED_CELLS: Cells = 0xf56a;

/// Which encoding a track is recorded in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Encoding {
    Fm,
    Mfm,
}

/// Encodes one byte's data bits as cells under `encoding`.
///
/// `previous` is the data bit recorded before this byte, which MFM's
/// clock rule reads and FM ignores. It is a parameter rather than state
/// because that is the whole of the history either rule needs.
pub(crate) fn encode_byte(encoding: Encoding, byte: u8, previous: bool) -> Cells {
    let mut cells: Cells = 0;
    let mut last = previous;
    for index in (0..8).rev() {
        let data = byte >> index & 1 == 1;
        let clock = match encoding {
            // FM clocks every cell pair, unconditionally.
            Encoding::Fm => true,
            // MFM clocks only between two zeroes.
            Encoding::Mfm => !last && !data,
        };
        cells = cells << 2 | Cells::from(clock) << 1 | Cells::from(data);
        last = data;
    }
    cells
}

/// Encodes a run of bytes, returning the cells and the last data bit —
/// which the next run needs to continue MFM's rule across the join.
pub(crate) fn encode(encoding: Encoding, bytes: &[u8], previous: bool) -> (Vec<bool>, bool) {
    let mut out = Vec::with_capacity(bytes.len() * 16);
    let mut last = previous;
    for byte in bytes {
        let cells = encode_byte(encoding, *byte, last);
        for index in (0..16).rev() {
            out.push(cells >> index & 1 == 1);
        }
        last = *byte & 1 == 1;
    }
    (out, last)
}

/// Lays down the cells of one address mark exactly, violations included.
pub(crate) fn encode_mark(cells: Cells) -> Vec<bool> {
    (0..16).rev().map(|index| cells >> index & 1 == 1).collect()
}

/// Reads the data bits out of sixteen cells, discarding the clocks.
///
/// It applies no rule and checks nothing: the caller has already decided
/// what this run of cells is, and a mark's illegal clocks decode to the
/// mark's own byte value here exactly as a lawful byte's do.
pub(crate) fn decode_byte(cells: Cells) -> u8 {
    let mut byte = 0u8;
    for index in (0..8).rev() {
        byte = byte << 1 | ((cells >> (index * 2)) & 1) as u8;
    }
    byte
}

/// Reads sixteen cells out of `bits` at `at`, or `None` past the end.
pub(crate) fn cells_at(bits: &[bool], at: usize) -> Option<Cells> {
    if at + 16 > bits.len() {
        return None;
    }
    let mut cells: Cells = 0;
    for offset in 0..16 {
        cells = cells << 1 | Cells::from(bits[at + offset]);
    }
    Some(cells)
}

/// Every position in `bits` where the sixteen cells are exactly `mark`.
///
/// The search is over cells rather than bytes and is not byte-aligned,
/// because nothing has established a byte boundary yet — locating the
/// mark is what establishes one.
pub(crate) fn find_marks(bits: &[bool], mark: Cells) -> Vec<usize> {
    let mut found = Vec::new();
    if bits.len() < 16 {
        return found;
    }
    let mut window: Cells = 0;
    for index in 0..bits.len() {
        window = window << 1 | Cells::from(bits[index]);
        if index >= 15 && window == mark {
            found.push(index - 15);
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mfm_clocks_only_between_two_zero_bits() {
        // 0x00 is all zeroes, so every pair carries a clock: the cells
        // alternate clock-set, data-clear.
        assert_eq!(encode_byte(Encoding::Mfm, 0x00, false), 0xaaaa);
        // 0xff is all ones, so no pair carries one at all.
        assert_eq!(encode_byte(Encoding::Mfm, 0xff, false), 0x5555);
    }

    #[test]
    fn mfm_reads_the_previous_bit_across_a_byte_boundary() {
        // The top bit of 0x00 follows whatever came before. After a one
        // there is no clock on that first pair; after a zero there is.
        let after_one = encode_byte(Encoding::Mfm, 0x00, true);
        let after_zero = encode_byte(Encoding::Mfm, 0x00, false);
        assert_ne!(after_one, after_zero);
        assert_eq!(after_one >> 14, 0b00, "no clock, no data");
        assert_eq!(after_zero >> 14, 0b10, "clock, no data");
        // And the rest of the byte is identical either way.
        assert_eq!(after_one & 0x3fff, after_zero & 0x3fff);
    }

    #[test]
    fn fm_clocks_every_cell_pair_whatever_the_data() {
        assert_eq!(encode_byte(Encoding::Fm, 0x00, false), 0xaaaa);
        assert_eq!(encode_byte(Encoding::Fm, 0xff, false), 0xffff);
        assert_eq!(
            encode_byte(Encoding::Fm, 0x00, true),
            encode_byte(Encoding::Fm, 0x00, false),
            "FM's rule reads no history"
        );
    }

    #[test]
    fn every_byte_survives_the_round_trip_in_both_encodings() {
        for encoding in [Encoding::Fm, Encoding::Mfm] {
            for previous in [false, true] {
                for value in 0..=255u8 {
                    assert_eq!(
                        decode_byte(encode_byte(encoding, value, previous)),
                        value,
                        "{encoding:?} {value:#04x} after {previous}"
                    );
                }
            }
        }
    }

    #[test]
    fn the_a1_mark_is_not_a_lawful_encoding_of_a1() {
        // This is the fact the whole framing rests on. The mark decodes
        // to the same byte and is a different cell pattern, and nothing
        // a recording could legally write produces it.
        let lawful = encode_byte(Encoding::Mfm, MFM_A1, false);
        assert_eq!(decode_byte(MFM_A1_CELLS), MFM_A1);
        assert_eq!(decode_byte(lawful), MFM_A1);
        assert_ne!(MFM_A1_CELLS, lawful);

        // Precisely one clock cell differs, and it is the one between the
        // two adjacent zero bits.
        assert_eq!((MFM_A1_CELLS ^ lawful).count_ones(), 1);
    }

    #[test]
    fn no_lawful_byte_in_any_history_encodes_to_the_mark() {
        for previous in [false, true] {
            for value in 0..=255u8 {
                assert_ne!(
                    encode_byte(Encoding::Mfm, value, previous),
                    MFM_A1_CELLS,
                    "{value:#04x} after {previous} would be indistinguishable from a mark"
                );
            }
        }
    }

    #[test]
    fn the_fm_marks_are_distinct_and_decode_to_their_own_bytes() {
        assert_eq!(decode_byte(FM_INDEX_CELLS), 0xfc);
        assert_eq!(decode_byte(FM_ID_CELLS), 0xfe);
        assert_eq!(decode_byte(FM_DATA_CELLS), 0xfb);
        assert_eq!(decode_byte(FM_DELETED_CELLS), 0xf8);

        let marks = [FM_INDEX_CELLS, FM_ID_CELLS, FM_DATA_CELLS, FM_DELETED_CELLS];
        for (at, left) in marks.iter().enumerate() {
            for right in &marks[at + 1..] {
                assert_ne!(left, right);
            }
            // Each is a violation: the lawful FM encoding of the same
            // byte clocks every pair, and these do not.
            let value = decode_byte(*left);
            assert_ne!(*left, encode_byte(Encoding::Fm, value, false));
        }
    }

    #[test]
    fn a_mark_is_found_at_the_cell_it_starts_at_and_not_on_a_byte_grid() {
        // Framing has not begun, so the mark may sit at any cell. Put it
        // at an offset that is not a multiple of sixteen and find it.
        let mut bits = vec![false; 5];
        bits.extend(encode_mark(MFM_A1_CELLS));
        bits.extend(encode(Encoding::Mfm, &[0xfe, 0x00], true).0);
        let found = find_marks(&bits, MFM_A1_CELLS);
        assert_eq!(found, vec![5]);
    }

    #[test]
    fn a_run_of_bytes_decodes_back_to_itself_through_the_cells() {
        let written: Vec<u8> = (0..64u8).map(|at| at.wrapping_mul(7)).collect();
        for encoding in [Encoding::Fm, Encoding::Mfm] {
            let (bits, _) = encode(encoding, &written, false);
            let read: Vec<u8> = (0..written.len())
                .map(|index| decode_byte(cells_at(&bits, index * 16).expect("in range")))
                .collect();
            assert_eq!(read, written, "{encoding:?}");
        }
    }
}
