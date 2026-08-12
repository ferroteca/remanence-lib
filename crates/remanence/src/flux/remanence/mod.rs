// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The flux family's physical stratum — the disk's own magnetization,
//! below every clock and every code.
//!
//! [`image`] is the model and its public root: form factor, holes as
//! angular data, and per surface the orbits, each an ordered circular
//! array of packed points held as cache-backed chunks, with the
//! invariants that govern them. [`format`] is the `.remanence`
//! artifact, claimed in both directions and reached through its own
//! type rather than a device (P13). [`reconstruction`] is the P29
//! reduction from an opened capture to an image under a declared
//! policy, answering with **the image itself** rather than a second
//! root beside it.

pub(crate) mod format;
pub(crate) mod image;
pub(crate) mod reconstruction;
