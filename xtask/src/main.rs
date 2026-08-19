// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! Builds `remanence-ffi`'s shipped and leak-probe cdylibs and prints the
//! CMake configure arguments `just test-ffi` needs to build and run the
//! C/C++ tests against them.
//!
//! **Kept in Rust, deliberately, and kept out of `remanence-ffi` itself.**
//! Which toolchain built the library is read from the file `cargo`
//! reported writing, not guessed from the host or from CMake's own
//! compiler search: MSVC cannot link a MinGW import library, MinGW cannot
//! link an MSVC one, and the two disagree about the C runtime besides.
//! Reimplementing that read in CMake would mean inferring it from the
//! host again — exactly the gap this design has always refused. It stays
//! out of `remanence-ffi` because that crate publishes to crates.io and
//! excludes `tests/**` on the principle that a released artifact carries
//! what a consumer runs and nothing else; a `[[bin]]` there would ship in
//! the tarball and force this crate's `serde_json` dependency into a real
//! one. `xtask` is a workspace member that is never a default one and is
//! never published.
//!
//! Two lines of output, meant to be read by a shell loop rather than a
//! person: `SLUG=<name>` once, naming the build directory `just test-ffi`
//! should configure into (a generator-specific name, so an MSVC shell and
//! an MSYS2 one can each keep a configured tree without evicting the
//! other's), and one `CMAKE_ARG=<token>` line per literal `cmake`
//! configure argument, already split the way `cmake`'s own argv wants it
//! (`-G` and its value are two lines, not one).

use std::path::{Path, PathBuf};
use std::process::Command;

/// Overrides the CMake generator, for a host where the default is wrong.
const GENERATOR: &str = "REMANENCE_CMAKE_GENERATOR";
/// Overrides the C compiler CMake would choose.
const CC_OVERRIDE: &str = "REMANENCE_CC";
/// The same, for C++.
const CXX_OVERRIDE: &str = "REMANENCE_CXX";

fn workspace_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .canonicalize()
        .expect("the workspace root is reachable from xtask's own crate directory")
}

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

/// A generator that can drive gcc.
///
/// CMake's default on Windows is a Visual Studio generator, which cannot:
/// it builds with MSVC or not at all, whatever `CMAKE_C_COMPILER` says.
fn mingw_generator() -> &'static str {
    for (tool, generator) in [("ninja", "Ninja"), ("mingw32-make", "MinGW Makefiles")] {
        let runs = Command::new(tool)
            .arg("--version")
            .output()
            .is_ok_and(|output| output.status.success());
        if runs {
            return generator;
        }
    }
    panic!(
        "the library was built by a MinGW toolchain, so the C tests have to \
         be built by one too — and neither `ninja` nor `mingw32-make` is on \
         PATH to drive it. Install one, or name a generator that works here \
         with {GENERATOR} (with {CC_OVERRIDE} / {CXX_OVERRIDE} for its \
         compilers)."
    );
}

/// A directory-safe name for a CMake generator.
///
/// `Visual Studio 18 2026` becomes `visual-studio-18-2026` and `Ninja`
/// becomes `ninja`, so the build tree says which generator wrote it and
/// two of them can sit side by side — CMake refuses to reconfigure a tree
/// a different generator wrote, so a shared directory would make an MSVC
/// shell and an MSYS2 shell take turns evicting each other. `None` — CMake
/// choosing unaided, which is the MSVC case — is `default` rather than an
/// empty suffix.
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

fn main() {
    let probe = build(
        "the probe-enabled library",
        &["leak-probe"],
        Some(&probe_target_dir()),
    );
    let shipped = build("the shipped library", &[], None);

    // The toolchain is not a preference. MSVC is what CMake finds unaided;
    // where the library is MSVC-built that is the native match. But a
    // MinGW import library is not something `cl.exe` can link, so naming
    // gcc is the only way the C/C++ tests meet the library at all. Both
    // overrides still win over this default.
    let mut generator = std::env::var(GENERATOR).ok();
    let mut cc = std::env::var(CC_OVERRIDE).ok();
    let mut cxx = std::env::var(CXX_OVERRIDE).ok();
    if shipped.toolchain == Toolchain::MinGw {
        generator.get_or_insert_with(|| mingw_generator().to_owned());
        cc.get_or_insert_with(|| "gcc".to_owned());
        cxx.get_or_insert_with(|| "g++".to_owned());
    }

    println!("SLUG={}", generator_slug(generator.as_deref()));

    let arg = |token: String| println!("CMAKE_ARG={token}");
    arg(format!("-DREMANENCE_LIB={}", shipped.link.display()));
    arg(format!("-DREMANENCE_PROBE_LIB={}", probe.link.display()));
    if let Some(runtime) = &shipped.runtime {
        arg(format!("-DREMANENCE_RUNTIME={}", runtime.display()));
    }
    if let Some(runtime) = &probe.runtime {
        arg(format!("-DREMANENCE_PROBE_RUNTIME={}", runtime.display()));
    }
    if let Some(generator) = &generator {
        arg("-G".to_owned());
        arg(generator.clone());
    }
    if let Some(cc) = &cc {
        arg(format!("-DCMAKE_C_COMPILER={cc}"));
    }
    if let Some(cxx) = &cxx {
        arg(format!("-DCMAKE_CXX_COMPILER={cxx}"));
    }
}
