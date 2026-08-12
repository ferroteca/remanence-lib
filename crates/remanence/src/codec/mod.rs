// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The compression codecs the library owns (P1).
//!
//! Self-contained by construction: an RFC 1951 DEFLATE decoder and
//! encoder and an LZMA/LZMA2 decoder, all written here rather than
//! taken from a dependency, and all streaming — a coded entry decodes
//! through its decompressor's LZ window into private session storage
//! and is never resident whole.

pub(crate) mod deflate;
pub(crate) mod inflate;
pub(crate) mod lzma;
