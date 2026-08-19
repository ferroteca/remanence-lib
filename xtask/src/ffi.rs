// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! `xtask ffi`: builds `remanence-ffi`'s shipped and leak-probe cdylibs,
//! refuses a MinGW/MSYS2 one outright (D66), and configures+builds the
//! CMake C/C++ test project against them — printing only the resulting
//! build directory, for `Taskfile.yml`'s `test-ffi` task to hand straight
//! to `ctest`.
//!
//! **Which toolchain built the library is read from the file `cargo`
//! reported writing, not guessed from the host or from CMake's own
//! compiler search.** On Windows this is what tells a native MSVC build
//! (`.dll.lib`) apart from a MinGW one (`.dll.a`), which this project no
//! longer supports building the C/C++ tests against at all — MSVC cannot
//! link a MinGW import library and the two disagree about the C runtime
//! besides, so a MinGW build is refused with a clear message rather than
//! accommodated. Linux and macOS builds (`.so`/`.dylib`) are a third,
//! unaffected case — no import-library split exists there, so there is
//! nothing to refuse or adapt to. Reimplementing this read in CMake would
//! mean inferring it from the host again — exactly the gap this design
//! has always refused.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::workspace_dir;

/// Overrides the CMake generator, for a host where the default is wrong.
const GENERATOR: &str = "REMANENCE_CMAKE_GENERATOR";
/// Overrides the C compiler CMake would choose.
const CC_OVERRIDE: &str = "REMANENCE_CC";
/// The same, for C++.
const CXX_OVERRIDE: &str = "REMANENCE_CXX";

/// Which C toolchain built the library.
///
/// Read from what a build produced rather than from the host's default,
/// because it is a claim about one file: MSVC cannot link a MinGW import
/// library, MinGW cannot link an MSVC one, and the two do not agree about
/// the C runtime either. Whatever cargo just wrote is what a C caller has
/// to be built against.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Toolchain {
    Msvc,
    /// Not accommodated — `run` refuses a build in this toolchain
    /// outright, since this project claims only the native Windows one.
    MinGw,
    /// Everywhere the shared object is linked directly, so there is no
    /// import library for two toolchains to disagree over.
    Native,
}

/// The suffixes a C caller can link, and what each is evidence of.
///
/// **The `.dll.` infix is load-bearing.** The static library sits beside
/// the import library in both Windows shapes — `remanence_ffi.lib` beside
/// MSVC's `remanence_ffi.dll.lib`, `libremanence_ffi.a` beside MinGW's
/// `libremanence_ffi.dll.a` — and each pair differs by that and nothing
/// else, so matching the last extension alone would link the wrong one of
/// the two half the time.
const LINKABLE: [(&str, Toolchain); 4] = [
    (".dll.lib", Toolchain::Msvc),
    (".dll.a", Toolchain::MinGw),
    (".so", Toolchain::Native),
    (".dylib", Toolchain::Native),
];

/// The suffixes of a file that has to be loadable when the caller runs.
const LOADABLE: [&str; 3] = [".dll", ".so", ".dylib"];

/// What one build of the library produced, **as cargo reported it**.
struct Built {
    /// What a C caller links against.
    link: PathBuf,
    /// What has to sit beside the executable at run time, where that is a
    /// different file from the one it links — which on Windows it always
    /// is, and elsewhere never.
    runtime: Option<PathBuf>,
    toolchain: Toolchain,
}

fn named(path: &Path, suffix: &str) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with(suffix))
}

fn listed(paths: &[PathBuf]) -> String {
    paths
        .iter()
        .map(|path| format!("  {}\n", path.display()))
        .collect()
}

/// Sorts one build's reported files into the two that matter.
fn classify(what: &str, filenames: &[PathBuf]) -> Built {
    let (link, toolchain) = LINKABLE
        .iter()
        .find_map(|(suffix, toolchain)| {
            let path = filenames.iter().find(|path| named(path, suffix))?;
            Some((path.clone(), *toolchain))
        })
        .unwrap_or_else(|| {
            panic!(
                "{what} reported no file a C caller could link against. It wrote:\n{}",
                listed(filenames)
            )
        });
    let runtime = filenames
        .iter()
        .find(|path| LOADABLE.iter().any(|suffix| named(path, suffix)))
        .filter(|path| **path != link)
        .cloned();
    Built {
        link,
        runtime,
        toolchain,
    }
}

/// The files a `cargo build` wrote for the cdylib, from its own report.
fn reported(stdout: &[u8]) -> Vec<PathBuf> {
    String::from_utf8_lossy(stdout)
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|message| message["reason"].as_str() == Some("compiler-artifact"))
        .filter(|message| {
            message["target"]["kind"]
                .as_array()
                .is_some_and(|kinds| kinds.iter().any(|kind| kind.as_str() == Some("cdylib")))
        })
        .flat_map(|message| {
            message["filenames"]
                .as_array()
                .map(|files| {
                    files
                        .iter()
                        .filter_map(|file| file.as_str().map(PathBuf::from))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        })
        .collect()
}

/// Builds `remanence-ffi` and answers what cargo reported writing.
///
/// **No fallback and no caching.** Both existed in the Rust harness this
/// replaces only to dodge the lock `cargo test` held on `target/<profile>`
/// while it ran; `xtask` is the sole top-level step here (build, then
/// configure, then build, then test, one at a time), so there is no lock
/// to dodge and nothing to memoize across a single run.
fn build(what: &str, features: &[&str], target_dir: Option<&Path>) -> Built {
    eprintln!("xtask: building {what}...");
    let mut command = Command::new("cargo");
    command.args(["build", "-p", "remanence-ffi"]);
    for feature in features {
        command.args(["--features", feature]);
    }
    command.args(["--message-format", "json-render-diagnostics"]);
    if let Some(target_dir) = target_dir {
        command.arg("--target-dir").arg(target_dir);
    }
    let output = command
        .current_dir(workspace_dir())
        .output()
        .unwrap_or_else(|error| panic!("cannot build {what}: {error}"));
    assert!(
        output.status.success(),
        "{what} did not build:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    classify(what, &reported(&output.stdout))
}

/// Where the probe-enabled build of the library goes.
///
/// **A separate target directory, which is the whole trick** (D50). The
/// leak probe is a global allocator and an exported symbol, so it must
/// never reach the shipped cdylib — an extra `remanence_*` symbol is an S2
/// change.
fn probe_target_dir() -> PathBuf {
    workspace_dir().join("target/leak-probe")
}

/// A directory-safe name for a CMake generator.
///
/// `Visual Studio 18 2026` becomes `visual-studio-18-2026` and `Ninja`
/// becomes `ninja`, so the build tree says which generator wrote it and
/// two of them can sit side by side — CMake refuses to reconfigure a tree
/// a different generator wrote, so a shared directory would make switching
/// `REMANENCE_CMAKE_GENERATOR` evict whatever was configured before. `None`
/// — CMake choosing unaided, which is the ordinary case — is `default`
/// rather than an empty suffix.
fn generator_slug(generator: Option<&str>) -> String {
    let Some(generator) = generator else {
        return "default".to_owned();
    };
    let mut slug = String::new();
    for character in generator.chars() {
        if character.is_ascii_alphanumeric() {
            slug.push(character.to_ascii_lowercase());
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug = slug.trim_matches('-');
    if slug.is_empty() {
        "default".to_owned()
    } else {
        slug.to_owned()
    }
}

/// Runs a `cmake` step to completion, panicking with its combined output
/// on failure. Output is not streamed on success, matching `build`'s own
/// cargo invocations above — `xtask` already reports what it is doing via
/// `eprintln!` before each long step starts.
fn run_cmake(what: &str, args: &[String]) {
    eprintln!("xtask: {what}...");
    let output = Command::new("cmake")
        .args(args)
        .output()
        .unwrap_or_else(|error| {
            panic!("cannot run cmake ({what}): {error}\n\nCMake drives the C tests. Install it.")
        });
    assert!(
        output.status.success(),
        "{what} failed:\n{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

pub fn run() {
    let probe = build(
        "the probe-enabled library",
        &["leak-probe"],
        Some(&probe_target_dir()),
    );
    let shipped = build("the shipped library", &[], None);

    // A MinGW/MSYS2 build is refused, not accommodated. This project no
    // longer treats a `windows-gnu` build as a legitimate alternative
    // toolchain (reversing a58247c's stance for remanence-py, and the
    // matching-not-preferring reasoning D46 built around this crate) —
    // MSVC is the one supported Windows toolchain, so there is nothing to
    // adapt CMake's generator or compiler to here. This is still read from
    // what the build actually produced, never guessed from the host: the
    // claim is specifically that *this* file is a MinGW import library.
    if shipped.toolchain == Toolchain::MinGw {
        panic!(
            "the shipped library was built by a MinGW/MSYS2 toolchain \
             (cargo wrote {}), which this project no longer supports. \
             Build with the native `x86_64-pc-windows-msvc` toolchain \
             instead — check that an MSYS2 shell's own `cc`/`link.exe` \
             is not resolving on `PATH` ahead of MSVC's, or build from a \
             shell where it does not (a plain PowerShell/cmd prompt, or \
             a Developer Command Prompt for Visual Studio).",
            shipped.link.display()
        );
    }

    // Both optional, and only relevant beyond the ordinary MSVC default
    // now that MinGW is no longer accommodated automatically — forcing a
    // faster generator (e.g. `-G Ninja` with `clang-cl`) is still a
    // legitimate reason to reach for either.
    let generator = std::env::var(GENERATOR).ok();
    let cc = std::env::var(CC_OVERRIDE).ok();
    let cxx = std::env::var(CXX_OVERRIDE).ok();

    let workspace = workspace_dir();
    let crate_dir = workspace.join("crates").join("remanence-ffi");
    let build_dir = workspace
        .join("target")
        .join(format!("c-tests-{}", generator_slug(generator.as_deref())));

    let mut configure_args = vec![
        "-S".to_owned(),
        crate_dir.join("c").join("tests").display().to_string(),
        "-B".to_owned(),
        build_dir.display().to_string(),
        format!("-DREMANENCE_LIB={}", shipped.link.display()),
        format!("-DREMANENCE_PROBE_LIB={}", probe.link.display()),
        format!(
            "-DREMANENCE_INCLUDE={}",
            crate_dir.join("c").join("include").display()
        ),
        format!(
            "-DREMANENCE_EXAMPLES={}",
            crate_dir.join("c").join("examples").display()
        ),
    ];
    if let Some(runtime) = &shipped.runtime {
        configure_args.push(format!("-DREMANENCE_RUNTIME={}", runtime.display()));
    }
    if let Some(runtime) = &probe.runtime {
        configure_args.push(format!("-DREMANENCE_PROBE_RUNTIME={}", runtime.display()));
    }
    if let Some(generator) = &generator {
        configure_args.push("-G".to_owned());
        configure_args.push(generator.clone());
    }
    if let Some(cc) = &cc {
        configure_args.push(format!("-DCMAKE_C_COMPILER={cc}"));
    }
    if let Some(cxx) = &cxx {
        configure_args.push(format!("-DCMAKE_CXX_COMPILER={cxx}"));
    }

    run_cmake("configuring the C/C++ test project", &configure_args);
    run_cmake(
        "building the C/C++ test project",
        &[
            "--build".to_owned(),
            build_dir.display().to_string(),
            "--config".to_owned(),
            "Debug".to_owned(),
        ],
    );

    // The one line on stdout: the build directory, for `ctest --test-dir`.
    println!("{}", build_dir.display());
}
