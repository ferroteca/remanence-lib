// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! Block image formats — implementations at representation seams (P12).
//!
//! [`adapters`] is the catalog and the wiring: the executable
//! image-format adapters, probe aggregation, the authoritative and
//! active layer vocabulary, and each format's declared recorded device
//! types. [`qcow2`] and [`vdi`] are the two native drivers beneath it,
//! each taking its P8 version gate first and refusing by name
//! everything outside its enumerated claim.
//!
//! The flux family's formats are **not** here: block and flux are
//! disjoint (P13), so a flux artifact is reached through its own type
//! rather than by opening a byte-addressed device. See [`crate::flux`].

pub(crate) mod adapters;
pub(crate) mod qcow2;
pub(crate) mod vdi;
