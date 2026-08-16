// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The flux family: magnetic recording descended to timed flux
//! transitions (P22), and the ladder that reads back up from them.
//!
//! **The family holds two models.** [`capture`] is what an instrument
//! recorded — locations, capture runs, circular observations, exact
//! timebases and the section-addressable backing they stream into —
//! with [`kryoflux`] the capture-set adapter above it. [`medium`] is
//! what a drive would read: one circular pulse stream per
//! family-addressed location, always derived and never constructible
//! without the policy that produced it. [`analysis`] is the numeric
//! core between them, over plain arrays.
//!
//! [`drive_profile`] is the P30 seam every rung reads its rules
//! through, so the layers take no policy arguments: being a medium of a
//! declared family *is* the declaration of how it is read.
//!
//! The P23 layers above the medium are [`bitstream`] — circular
//! track-relative clocked bit state, every bit saying whether it was
//! recorded or resolved by a declared rule — and [`bytestream`], the
//! byte sequence a declared code makes of it, which assigns no
//! header, sector or file to any of them. [`presentation`] is the seam
//! a caller reaches them through and the phase-locked channel that
//! produces the first: **the rungs name no family**, and the transition
//! above the bits is the profile's own behavior rather than a branch
//! here. [`c1541`] is the one family carried through to a filesystem,
//! and [`remanence`] the physical stratum beneath the whole ladder.
//!
//! [`p64`] is the served form, claimed in both directions, and
//! [`load`] is the single seam into the session model — the two
//! declared flux loads and the provenance they ride onto a pooled
//! medium. Everything else here reaches the core only for bytes, a
//! claim and a cache bound.

pub(crate) mod analysis;
pub(crate) mod bitstream;
pub(crate) mod bytestream;
pub(crate) mod c1541;
pub(crate) mod capture;
pub(crate) mod drive_profile;
pub(crate) mod kryoflux;
pub(crate) mod load;
pub(crate) mod medium;
pub(crate) mod p64;
pub(crate) mod presentation;
pub(crate) mod remanence;
