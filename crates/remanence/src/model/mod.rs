// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The session model: the pools, the nodes a caller holds, and the
//! three fact classes that fill them.
//!
//! [`pools`] owns the session, its device set and its media pool (P32)
//! — media as state, devices as configuration — with [`storage_device`]
//! the slot that links them and [`media`] the medium a caller holds,
//! over the private [`disk`] state it homes.
//!
//! The three fact classes meet here. **Discovery** reads
//! ([`discovery`], answering on no handle at all), **declaration**
//! configures ([`device_type`], the P14 recording seam, beside
//! [`media_profile`], the P14 substrate seam), and **authorship**
//! creates media whole ([`authored`]).
//!
//! What an open established travels with the medium: [`assurance`] is
//! the P28 gate, [`geometry`] the recording's own coordinates as
//! evidence, [`session`] the layered identification model, [`report`]
//! the inspection records, and [`volume`] the P17 composition seam.

pub(crate) mod assurance;
pub(crate) mod authored;
pub(crate) mod device_type;
pub(crate) mod discovery;
pub(crate) mod disk;
pub(crate) mod geometry;
pub(crate) mod media;
pub(crate) mod media_profile;
pub(crate) mod pools;
pub(crate) mod report;
pub(crate) mod session;
pub(crate) mod storage_device;
pub(crate) mod volume;
