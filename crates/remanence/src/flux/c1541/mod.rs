// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The Commodore 1541 family above the P23 layers.
//!
//! [`presentation`] is the family's read channel and GCR codec — the
//! declared policies of each transition read argument-free through the
//! type (P30), the clocking, the framing, and the account of what each
//! layer does not carry from the one below. [`sectors`] is the rung
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

pub(crate) mod presentation;
pub(crate) mod renditions;
pub(crate) mod sectors;
