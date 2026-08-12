// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! Bytes, claims and the bound over them — everything above this reads
//! and writes through here.
//!
//! [`handle`] is the caller-owned claim (P7): what a handed-over
//! `std::fs::File` affords, and the name recovered from it for location
//! alone. [`device`] is the block-device seam and the `Claim` class that
//! says whose open a medium's is. [`cache`] is the P2 commit buffer and
//! the P27 bounded working set, [`journal`] the durable commit's
//! intent record (P9), and [`source`] resolves a file named by path or
//! one entry named through an archive medium's namespace.

pub(crate) mod cache;
pub(crate) mod device;
pub(crate) mod handle;
pub(crate) mod journal;
pub(crate) mod source;
