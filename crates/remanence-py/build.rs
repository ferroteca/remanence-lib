// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! Which Python this module is built against: checked before anything
//! compiles, and recorded for the suite that tests it.
//!
//! **The claim is native CPython** (S3). The wheels this project
//! publishes are abi3 for the interpreter python.org and uv distribute,
//! and that is the only Python a released artifact is ever loaded by. An
//! interpreter outside the claim is refused here by name, rather than
//! quietly producing a module that imports nowhere the project ships to.
//!
//! What provokes it is ordinary. MSYS2 puts its own Python first on
//! `PATH`; pyo3 asks whichever `python` it finds; MSYS2's reports
//! `mingw_x86_64_ucrt` as its platform. pyo3 then names the import
//! library `libpython3` rather than `python3`, which takes it out of the
//! subset `raw-dylib` linking covers, so the module links
//! `libpython3.dll` — a DLL that exists only inside MSYS2. It imports
//! there and nowhere else, and until this check nothing downstream said
//! so: the wheel built, published, and failed at `import` on every
//! consumer's machine. Exactly that shipped once, as 0.0.1a4.
//!
//! **`uv build` never trips this**, because it hands maturin a native
//! interpreter of its own. What reaches it is a `cargo build` from a
//! shell whose `PATH` leads somewhere else — which is the ordinary way
//! to work on this tree from MSYS2, and why the refusal names its own
//! remedy rather than only stating the rule.

fn main() {
    // Our answer depends on which interpreter pyo3 resolved, and that
    // is the variable that decides it. pyo3's own script asks for the
    // same rerun; this one needs it on its own account.
    println!("cargo:rerun-if-env-changed=PYO3_PYTHON");

    let config = pyo3_build_config::get();
    let interpreter = config.executable().unwrap_or("<unstated>");

    // Only Windows draws this distinction: the "lib" prefix is how a
    // MinGW Python names its import library, where the native one is
    // bare `python3`. Elsewhere every interpreter is lib-prefixed and
    // the prefix says nothing.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        if let Some(lib_name) = config.lib_name() {
            assert!(
                !lib_name.starts_with("lib"),
                "\n\nthis module would be built against\n\n  {interpreter}\n\n\
                 which is a MinGW Python: pyo3 named its import library \
                 `{lib_name}` rather than `python3`, so the module would link \
                 `libpython3.dll` and import only inside MSYS2. The wheels \
                 this project publishes are for the native CPython that \
                 python.org and uv distribute, which has no such DLL — a \
                 module built this way fails at `import` for every consumer, \
                 which is what 0.0.1a4 did.\n\n\
                 Point the build at a native interpreter:\n\n  \
                 export PYO3_PYTHON=$(uv python find)\n\n\
                 `uv build crates/remanence-py` needs none of this — it \
                 supplies its own native interpreter — so the wheel is \
                 unaffected by all of this.\n"
            );
        }
    }

    // Named so a failing suite can say which Python the module it is
    // testing was actually built for. Every target of this package sees
    // it, tests included.
    println!("cargo:rustc-env=REMANENCE_BUILD_INTERPRETER={interpreter}");
}
