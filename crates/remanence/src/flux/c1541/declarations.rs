// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! What the Commodore 1541 declares about its own recording, and what
//! nothing else does.
//!
//! These four sat in the shared [`Presentation`] struct while the 1541
//! was the only family in it. A group-code table, a record grammar and
//! the policies over them are not facts every family has — an FM or MFM
//! recording has no symbol table, and its framing is an encoding
//! violation rather than a run of one-bits — so a second family could
//! only have filled them in with values that meant nothing.
//!
//! They live here instead, read by the transition this family enrols on
//! its profile. Nothing outside `c1541` refers to them.
//!
//! [`Presentation`]: crate::flux::drive_profile::Presentation

use crate::flux::c1541::presentation::GcrCodecPolicy;
use crate::flux::c1541::sectors::SectorPolicy;
use crate::flux::drive_profile::{BlockShape, ChecksumRule, GroupCodec, RecordGrammar};

/// The symbol each four-bit value is recorded as, indexed by the value.
///
/// It is a declared fact of the family in exactly the sense every other
/// field of the profile is: a published table, not a pattern any
/// recording is permitted to establish.
static C1541_GCR_SYMBOLS: [u16; 16] = [
    0b01010, 0b01011, 0b10010, 0b10011, 0b01110, 0b01111, 0b10110, 0b10111, 0b01001, 0b11001,
    0b11010, 0b11011, 0b01101, 0b11101, 0b11110, 0b10101,
];

pub(crate) static CODEC: GroupCodec = GroupCodec {
    id: "c1541-gcr",
    name: "Commodore group-coded recording",
    symbol_bits: 5,
    data_bits: 4,
    symbols: &C1541_GCR_SYMBOLS,
    provenance: "declared from the published Commodore GCR table: each four-bit \
                     value is recorded as one of sixteen five-bit symbols, chosen so \
                     that no symbol and no pair of them runs more than two zeros or \
                     four ones together",
};

pub(crate) static RECORD: RecordGrammar = RecordGrammar {
    id: "cbm-dos-record",
    name: "CBM DOS sector record",
    checksum: ChecksumRule::Xor,
    header: BlockShape {
        id: "header",
        mark: 0x08,
        bytes: 8,
        checksum_at: 1,
        checked_from: 2,
        checked_to: 6,
    },
    data: BlockShape {
        id: "data",
        mark: 0x07,
        bytes: 260,
        checksum_at: 257,
        checked_from: 1,
        checked_to: 257,
    },
    track_at: 3,
    sector_at: 2,
    id_high_at: 5,
    id_low_at: 4,
    payload_from: 1,
    payload_to: 257,
    provenance: "declared from the published CBM DOS recording conventions: a \
                     sector is written as an eight-byte header block opening 0x08, \
                     holding the sector, the track and the two disk-identity bytes \
                     with their checksum, and then — after a gap and a second sync — \
                     a 260-byte data block opening 0x07, holding 256 payload bytes \
                     and their checksum",
};

pub(crate) static CODEC_POLICY: GcrCodecPolicy = GcrCodecPolicy {
    // Framing begins at the family's declared landmark and
    // nowhere else: bits before the first sync are unframed
    // rather than guessed into bytes.
    alignment: crate::flux::c1541::presentation::AlignmentPolicy::Landmark,
    // A pattern the table does not assign keeps its own bits,
    // stated as unresolved and counted.
    unassigned_symbol: crate::flux::c1541::presentation::UnassignedSymbolPolicy::DeclareLoss,
};

pub(crate) static SECTOR_POLICY: SectorPolicy = SectorPolicy {
    checksum_failure: crate::flux::c1541::sectors::ChecksumFailurePolicy::DeclareLoss,
    unpaired_record: crate::flux::c1541::sectors::UnpairedRecordPolicy::DeclareLoss,
};
