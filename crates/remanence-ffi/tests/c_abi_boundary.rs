// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The C ABI, exercised from C (S2, D45, D46).
//!
//! The FFI crate's unit tests call the `extern "C"` functions **from
//! Rust**. That checks the logic behind them and nothing about the
//! boundary: no header is included, no C compiler sees a declaration, no
//! C calling convention is used, and a `#[repr(C)]` mistake cannot show.
//! D44 closed half the gap by compiling the header and the example; this
//! closes the other half by linking and running.
//!
//! `tests/c/abi_boundary.c` is the caller. It takes a group name, so
//! each group is a named test here rather than one pass-or-fail lump,
//! and it keeps going after a failed check so one run reports everything
//! wrong with a group.
//!
//! **These need the built library, which `cargo test` does not produce.**
//! `cargo build` does, and AGENTS.md orders it first, so the ordinary
//! flow satisfies it.
//!
//! **Everything here builds what it needs or asks the library about
//! itself**, so a fresh clone runs the whole file. The one group that
//! wanted a generated disk is `c_abi_rig.rs`, behind the `rigs` feature.

mod common;

use common::run_c;

fn group(name: &str, args: &[&str]) {
    print!("{}", run_c("abi_boundary", &[&[name], args].concat()));
}

#[test]
fn the_catalogs_answer_across_the_boundary() {
    group("catalogs", &[]);
}

#[test]
fn the_version_and_cache_bound_cross() {
    group("version", &[]);
}

#[test]
fn a_refusal_sets_its_out_parameters() {
    group("refusal", &[]);
}

#[test]
fn accessors_answer_on_a_null_handle() {
    group("nulls", &[]);
}

/// The session's device set, as a C caller meets it: a slot is filled
/// once, an attachment resolves to the device in it, releasing frees the
/// slot, and every accessor answers on a null handle rather than
/// dereferencing it.
#[test]
fn the_device_set_crosses_the_boundary() {
    group("devices", &[]);
}
