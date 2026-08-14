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
//! **The C caller is built by whichever toolchain built the library.**
//! MSVC is what that means on the developing host, and is what the
//! paragraph above is about — but it is a consequence rather than the
//! rule. A `windows-gnu` rustc writes a MinGW import library, which
//! `cl.exe` cannot link and whose C runtime is not MSVC's, so on that
//! host CMake is pointed at gcc instead. What decides is the file
//! `cargo` reported writing, not the developing host's habits.
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

/// Which C toolchain built the library.
///
/// Read from what a build produced rather than from the host's default,
/// because it is a claim about one file: MSVC cannot link a MinGW import
/// library, MinGW cannot link an MSVC one, and the two do not agree
/// about the C runtime either. Whatever cargo just wrote is what a C
/// caller has to be built against.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Toolchain {
    Msvc,
    MinGw,
    /// Everywhere the shared object is linked directly, so there is no
    /// import library for two toolchains to disagree over.
    Native,
}

/// The suffixes a C caller can link, and what each is evidence of.
///
/// **The `.dll.` infix is load-bearing.** The static library sits beside
/// the import library in both Windows shapes — `remanence_ffi.lib`
/// beside MSVC's `remanence_ffi.dll.lib`, `libremanence_ffi.a` beside
/// MinGW's `libremanence_ffi.dll.a` — and each pair differs by that and
/// nothing else, so matching the last extension alone would link the
/// wrong one of the two half the time.
const LINKABLE: [(&str, Toolchain); 4] = [
    (".dll.lib", Toolchain::Msvc),
    (".dll.a", Toolchain::MinGw),
    (".so", Toolchain::Native),
    (".dylib", Toolchain::Native),
];

/// The suffixes of a file that has to be loadable when the caller runs.
const LOADABLE: [&str; 3] = [".dll", ".so", ".dylib"];

/// What one build of the library produced, **as cargo reported it**.
///
/// No name here was invented by this harness. Reading the file names out
/// of `--message-format=json` is what makes the lookup true on a host
/// whose artifacts are not shaped like the developing host's, and it is
/// also what keeps a *stale* one out of the link: a file left behind by
/// a toolchain this tree no longer uses cannot appear in a report of
/// what was just written, however plausible its name looks in a
/// directory listing.
pub struct Built {
    /// What a C caller links against.
    pub link: PathBuf,
    /// What has to sit beside the executable at run time, where that is
    /// a different file from the one it links — which on Windows it
    /// always is, and elsewhere never.
    pub runtime: Option<PathBuf>,
    /// The toolchain `link` is evidence of.
    pub toolchain: Toolchain,
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
                "{what} reported no file a C caller could link against. It \
                 wrote:\n{}",
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

/// What the shipped build produced, under the names the probe build has
/// just proved this toolchain uses.
///
/// **This is the one lookup that cannot ask cargo.** The `cargo test`
/// run in progress holds the lock on `target/<profile>`, so a nested
/// build there would wait on the run waiting for it. It takes the file
/// names from the probe build instead — same package and same toolchain,
/// so the same names, in a directory of its own — and looks for exactly
/// those. Nothing else in the directory is a candidate, which is what
/// keeps an artifact from a toolchain this tree no longer uses out of
/// the link.
fn shipped_build() -> &'static Built {
    static SHIPPED: OnceLock<Built> = OnceLock::new();
    SHIPPED.get_or_init(|| {
        let probe = probe_build();
        let dir = target_dir();
        let beside = |path: &Path| dir.join(path.file_name().expect("a built file has a name"));

        let link = beside(&probe.link);
        assert!(link.exists(), "{}", absent(&dir, &link));
        Built {
            runtime: probe
                .runtime
                .as_deref()
                .map(beside)
                .filter(|path| path.exists()),
            link,
            toolchain: probe.toolchain,
        }
    })
}

/// Why there is nothing to link, including the case worth naming: a
/// leftover from a toolchain this tree used to be built with.
fn absent(dir: &Path, expected: &Path) -> String {
    let name = expected.file_name().unwrap_or_default().to_string_lossy();
    let strangers: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| LINKABLE.iter().any(|(suffix, _)| named(path, suffix)))
        .filter(|path| path.file_name() != expected.file_name())
        .filter_map(|path| Some(path.file_name()?.to_string_lossy().into_owned()))
        .collect();
    let stale = if strangers.is_empty() {
        String::new()
    } else {
        format!(
            "\n\nIt does hold {}, which this toolchain does not produce — a \
             leftover from a build by another one. Cargo does not delete a \
             file it has stopped writing, and linking that would be wrong \
             rather than merely lucky.",
            strangers.join(", ")
        )
    };
    format!(
        "no `{name}` in {}, so there is nothing for a C caller to link \
         against.{stale}\n\n\
         `cargo test` does not build a cdylib — `cargo build` does, and \
         AGENTS.md orders it first for exactly this reason:\n\n  \
         cargo build\n  cargo test\n\n\
         This does not run cargo itself: a nested build would contend for \
         the lock the current run already holds.",
        dir.display()
    )
}

/// The DLL that must sit beside a test executable on Windows.
pub fn runtime_library() -> Option<PathBuf> {
    shipped_build().runtime.clone()
}

/// Where the probe-enabled build of the library goes.
///
/// **A separate target directory, which is the whole trick** (D50). The
/// leak probe is a global allocator and an exported symbol, so it must
/// never reach the shipped cdylib — an extra `remanence_*` symbol is an
/// S2 change. Building it here leaves `target/<profile>` untouched, and
/// because cargo locks a target directory rather than a workspace, this
/// build can run while `cargo test` holds the lock on the other one.
fn probe_target_dir() -> PathBuf {
    crate_dir().join("../../target/leak-probe")
}

/// Builds the library with the leak probe and answers what it wrote.
///
/// Cold this costs about twenty seconds; warm it is a no-op cargo
/// resolves in a fifth of a second, which is what makes running the leak
/// check by default affordable.
///
/// **The build reports its own output** rather than being searched for
/// afterwards. Where cargo puts a file and what it calls it are cargo's
/// to decide — a target triple in the path, a toolchain's naming — and
/// asking is exact where guessing is right only on the host it was
/// written on. `json-render-diagnostics` keeps that report on stdout and
/// leaves the compiler's own errors readable on stderr, so a build that
/// fails still says why below.
fn probe_build() -> &'static Built {
    static PROBE: OnceLock<Built> = OnceLock::new();
    PROBE.get_or_init(|| {
        let output = Command::new("cargo")
            .args([
                "build",
                "-p",
                "remanence-ffi",
                "--features",
                "leak-probe",
                "--message-format",
                "json-render-diagnostics",
            ])
            .arg("--target-dir")
            .arg(probe_target_dir())
            .current_dir(workspace_dir())
            .output()
            .unwrap_or_else(|error| {
                panic!(
                    "cannot build the probe-enabled library: {error}\n\n\
                     The leak check runs by default (D50) and needs a cdylib \
                     built with `--features leak-probe`, which never ships. \
                     Set {SKIP}=1 to skip the C tests deliberately."
                )
            });
        assert!(
            output.status.success(),
            "the probe-enabled library did not build:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        classify("the probe build", &reported(&output.stdout))
    })
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
        "the library was built by a MinGW toolchain, so the C tests have \
         to be built by one too — and neither `ninja` nor `mingw32-make` \
         is on PATH to drive it. Install one, name a generator that works \
         here with {GENERATOR} (with {CC_OVERRIDE} / {CXX_OVERRIDE} for \
         its compilers), or set {SKIP}=1 to skip the C tests deliberately."
    );
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
            .arg(format!(
                "-DREMANENCE_LIB={}",
                shipped_build().link.display()
            ))
            .arg(format!(
                "-DREMANENCE_INCLUDE={}",
                crate_dir().join("include").display()
            ))
            .arg(format!(
                "-DREMANENCE_EXAMPLES={}",
                crate_dir().join("examples").display()
            ));

        // The leak target links its own build of the library — the one
        // carrying the probe — so CMake is told about both.
        configure.arg(format!(
            "-DREMANENCE_PROBE_LIB={}",
            probe_build().link.display()
        ));

        // The toolchain is not a preference. MSVC is what CMake finds
        // unaided and what D46 is written around, and where the library
        // is MSVC-built that is the native match — but a MinGW import
        // library is not something `cl.exe` can link, so naming gcc
        // there is the only way the caller meets the library at all.
        // Both overrides still win: this only supplies a default where
        // CMake's own would be wrong.
        let mut generator = std::env::var_os(GENERATOR);
        let mut compilers = [
            (CC_OVERRIDE, "CMAKE_C_COMPILER", "gcc"),
            (CXX_OVERRIDE, "CMAKE_CXX_COMPILER", "g++"),
        ]
        .map(|(variable, cmake, mingw)| (cmake, std::env::var(variable).ok(), mingw));
        if shipped_build().toolchain == Toolchain::MinGw {
            generator.get_or_insert_with(|| mingw_generator().into());
            for (_, compiler, mingw) in &mut compilers {
                compiler.get_or_insert_with(|| (*mingw).to_owned());
            }
        }

        if let Some(generator) = generator {
            configure.arg("-G").arg(generator);
        }
        for (cmake, compiler, _) in &compilers {
            if let Some(compiler) = compiler {
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
        // Windows resolves an import from beside the executable before
        // anything on PATH, and both builds of the library carry the
        // same file name — so the probe binary gets its own directory
        // and its own copy, or it would load the shipped one and fail to
        // find the symbol.
        if let Some(library) = &shipped_build().runtime {
            let name = library.file_name().expect("the library has a name");
            let _ = std::fs::copy(library, bin.join(name));

            if let Some(probe) = &probe_build().runtime {
                let probe_bin = bin.join("leak");
                let _ = std::fs::create_dir_all(&probe_bin);
                let name = probe.file_name().expect("the library has a name");
                let _ = std::fs::copy(probe, probe_bin.join(name));
            }
        }
        bin
    })
}

/// Runs a built C executable with the given arguments, from the
/// workspace root so fixture paths in the tests are workspace-relative.
pub fn run_c(program: &str, args: &[&str]) -> String {
    run_c_in(build_dir().clone(), program, args)
}

/// The same, for the probe binary, which lives beside its own copy of
/// the library rather than the shipped one.
pub fn run_c_probe(program: &str, args: &[&str]) -> String {
    run_c_in(build_dir().join("leak"), program, args)
}

fn run_c_in(dir: PathBuf, program: &str, args: &[&str]) -> String {
    let exe = dir.join(if cfg!(windows) {
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
