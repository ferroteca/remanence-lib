// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

#![allow(dead_code)]

use std::fs::File;
use std::path::Path;

/// The caller's own read-only open — the source shape `load_media` takes
/// (P7 as amended: whoever opens owns the lock).
pub fn open_read(path: impl AsRef<Path>) -> File {
    let path = path.as_ref();
    File::open(path).unwrap_or_else(|error| panic!("cannot open '{}': {error}", path.display()))
}

/// The caller's own read/write open, which is what affords the library a
/// write.
pub fn open_write(path: impl AsRef<Path>) -> File {
    let path = path.as_ref();
    File::options()
        .read(true)
        .write(true)
        .open(path)
        .unwrap_or_else(|error| panic!("cannot open '{}' for writing: {error}", path.display()))
}
