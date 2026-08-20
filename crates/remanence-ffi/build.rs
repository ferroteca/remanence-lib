// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! Regenerates c/include/remanence.h from the crate's public C ABI.

fn main() {
    let crate_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");

    // The whole module tree, not just the root: the header is generated from
    // every `extern "C"` item in the crate, and most of them live in submodules.
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=cbindgen.toml");

    let config = cbindgen::Config::from_file(format!("{crate_dir}/cbindgen.toml"))
        .expect("cbindgen.toml parses");

    cbindgen::Builder::new()
        .with_crate(&crate_dir)
        .with_config(config)
        .generate()
        .expect("cbindgen generates the C header")
        .write_to_file(format!("{crate_dir}/c/include/remanence.h"));
}
