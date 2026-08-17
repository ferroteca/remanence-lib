// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The IBM System 3740 and System 34 recordings — FM and MFM — above the
//! P23 layers (F78).
//!
//! [`encoding`] is how the two encodings lay a byte down as clock and
//! data cells, and the address marks whose deliberate clock violations
//! are the only thing a recording can use to say a field begins here.
//! [`records`] is the layer above: the address each sector claims, the
//! data field that follows it, and both checksums stated beside
//! computed.
//!
//! **Both are arithmetic, and neither reads a profile.** They take cells
//! and an encoding and report what they find, which is what lets them be
//! checked by writing a track and reading it back rather than against an
//! artifact somebody else made.
//!
//! **Nothing calls them yet, and the `dead_code` allowance below says
//! so.** What is missing is the family: a drive profile declaring the
//! rotation, rate and density this channel is clocked at, and a format
//! that carries such a recording into a medium. Until one exists this is
//! a complete, tested layer with no caller — which is a deliberate
//! holding position rather than an oversight, and the allowance is
//! removed by the change that enrols the family.
#![allow(dead_code)]

pub(crate) mod encoding;
pub(crate) mod geometry;
pub(crate) mod presentation;
pub(crate) mod profiles;
pub(crate) mod records;
pub(crate) mod sectors;
