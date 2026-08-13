// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! Driving the C tests' CMake build, shared by everything that needs it.
//!
//! **CMake is here for MSVC**, and for nothing else it happens to also
//! do. `cl.exe` needs the environment `vcvars64.bat` sets, and locating
//! and sourcing that from a test harness is more bespoke machinery than
//! the compiler discovery it replaces — where MSYS2's gcc needed a known
//! install path, its own directory on `PATH`, and a rule against ever
//! trying a bare `cl` (which resolves to Watcom's on the development
//! host). CMake finds MSVC unaided and sets its environment up, so all
//! of that goes.
//!
//! Configured and built **once per test binary**: the harness runs tests
//! on threads of one process, and concurrent `cmake --build` calls on
//! one build directory race. A failure is reported to whichever test
//! asked first, with the compiler's own output.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

/// Overrides the CMake generator, for a host where the default is wrong.
pub const GENERATOR: &str = "REMANENCE_CMAKE_GENERATOR";
/// Overrides the C compiler CMake would choose.
pub const CC_OVERRIDE: &str = "REMANENCE_CC";
/// The same, for C++.
pub const CXX_OVERRIDE: &str = "REMANENCE_CXX";
/// Skips the C tests deliberately. An unrun check must be somebody's
/// decision rather than a tool's absence.
pub const SKIP: &str = "REMANENCE_SKIP_CC";
/// The build configuration asked of multi-config generators.
const CONFIG: &str = "Debug";

pub fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

pub fn workspace_dir() -> PathBuf {
    crate_dir()
        .join("../..")
        .canonicalize()
        .expect("the workspace root is reachable from the crate")
}

/// `target/<profile>`, from this test binary's own location rather than
/// assumed to be `debug`, so a `--release` run links what it just built.
pub fn target_dir() -> PathBuf {
    let exe = std::env::current_exe().expect("a test binary knows its own path");
    exe.parent()
        .and_then(Path::parent)
        .expect("the test binary sits in target/<profile>/deps")
        .to_path_buf()
}

pub fn skipping() -> bool {
    if std::env::var_os(SKIP).is_some() {
        eprintln!("!! {SKIP} is set: the C surface was NOT built or run.");
        return true;
    }
    false
}

/// The library a C caller links against: MSVC links the import library
/// beside the DLL, everything else links the shared object itself.
fn link_target() -> PathBuf {
    let dir = target_dir();
    for name in [
        "remanence_ffi.dll.lib",
        "libremanence_ffi.so",
        "libremanence_ffi.dylib",
    ] {
        let path = dir.join(name);
        if path.exists() {
            return path;
        }
    }
    panic!(
        "no built library in {}, so there is nothing for a C caller to \
         link against.\n\n\
         `cargo test` does not build a cdylib — `cargo build` does, and \
         AGENTS.md orders it first for exactly this reason:\n\n  \
         cargo build\n  cargo test\n\n\
         This does not run cargo itself: a nested build would contend for \
         the lock the current run already holds.",
        dir.display()
    );
}

/// The DLL that must sit beside a test executable on Windows.
pub fn runtime_library() -> Option<PathBuf> {
    let path = target_dir().join("remanence_ffi.dll");
    path.exists().then_some(path)
}

fn run(what: &str, command: &mut Command) -> String {
    let output = command.output().unwrap_or_else(|error| {
        panic!(
            "cannot run {what}: {error}\n\n\
             CMake drives the C tests since D46. Install it, or set \
             {SKIP}=1 to skip them deliberately."
        )
    });
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.status.success(), "{what} failed:\n{text}");
    text
}

/// Where the built executables land, whichever generator ran.
static BUILD: OnceLock<PathBuf> = OnceLock::new();

/// Configures and builds every C target once, and answers the directory
/// the executables are in.
pub fn build_dir() -> &'static PathBuf {
    BUILD.get_or_init(|| {
        let source = crate_dir().join("tests/c");
        let build = target_dir().join("c-tests");

        let mut configure = Command::new("cmake");
        configure
            .arg("-S")
            .arg(&source)
            .arg("-B")
            .arg(&build)
            .arg(format!("-DREMANENCE_LIB={}", link_target().display()))
            .arg(format!(
                "-DREMANENCE_INCLUDE={}",
                crate_dir().join("include").display()
            ))
            .arg(format!(
                "-DREMANENCE_EXAMPLES={}",
                crate_dir().join("examples").display()
            ));

        // Stated either way, never omitted. CMake caches what it was
        // last told, so a build directory configured under
        // `--features leak-probe` would keep trying to link the probe
        // target on the next run without it — an unresolved symbol that
        // fails every C test for a reason none of them is about.
        configure.arg(if cfg!(feature = "leak-probe") {
            "-DREMANENCE_LEAK_PROBE=ON"
        } else {
            "-DREMANENCE_LEAK_PROBE=OFF"
        });
        if let Some(generator) = std::env::var_os(GENERATOR) {
            configure.arg("-G").arg(generator);
        }
        for (variable, cmake) in [
            (CC_OVERRIDE, "CMAKE_C_COMPILER"),
            (CXX_OVERRIDE, "CMAKE_CXX_COMPILER"),
        ] {
            if let Some(compiler) = std::env::var(variable).ok() {
                configure.arg(format!("-D{cmake}={compiler}"));
            }
        }

        run("cmake configure", &mut configure);
        run(
            "cmake build",
            Command::new("cmake")
                .arg("--build")
                .arg(&build)
                .arg("--config")
                .arg(CONFIG),
        );

        let bin = build.join("bin");
        // Windows resolves imports from beside the executable.
        if let Some(library) = runtime_library() {
            let beside = bin.join(library.file_name().expect("the library has a name"));
            let _ = std::fs::copy(&library, &beside);
        }
        bin
    })
}

/// Runs a built C executable with the given arguments, from the
/// workspace root so fixture paths in the tests are workspace-relative.
pub fn run_c(program: &str, args: &[&str]) -> String {
    let exe = build_dir().join(if cfg!(windows) {
        format!("{program}.exe")
    } else {
        program.to_owned()
    });
    assert!(
        exe.exists(),
        "the C target `{program}` was not built; expected it at {}",
        exe.display()
    );

    let output = Command::new(&exe)
        .args(args)
        .current_dir(workspace_dir())
        .output()
        .expect("the C caller runs");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.status.success(),
        "the C caller `{program} {}` failed. This is the ABI as C meets \
         it, so a failure here is a real boundary defect rather than a \
         harness one:\n{text}",
        args.join(" ")
    );
    text
}
