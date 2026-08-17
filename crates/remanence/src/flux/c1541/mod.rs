// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The Commodore 1541 family above the P23 layers.
//!
//! [`presentation`] is the family's GCR codec — the framing landmark,
//! the group-code table, and the account of what the byte layer does not
//! carry from the bits. The rung beneath it is *not* the family's: the
//! phase-locked channel that clocks pulses into cells reads every number
//! it uses off this profile and serves every enrolled family, so it
//! lives at [`crate::flux::presentation`], which is also where the two
//! family-neutral rungs are. [`sectors`] is the rung
//! above it, **the seam where the bytestream's silence ends**, and it
//! ends by stating what it derives: the family's declared record
//! grammar recognized over the bytestream's own runs, every claim
//! carrying both checksums stated beside computed.
//!
//! [`renditions`] masters the d64, g64 and p64 renditions off the
//! remanence image, each claimed twice and each stating what its
//! destination did not carry (P29). Its GCR and sector reading are
//! analysis machinery, deliberately **not** the sector surface
//! [`sectors`] presents.

pub(crate) mod declarations;
pub(crate) mod presentation;
pub(crate) mod renditions;
pub(crate) mod sectors;
