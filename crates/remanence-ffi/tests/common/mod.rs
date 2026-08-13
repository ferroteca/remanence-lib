// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! Finding and driving a C toolchain, shared by the tests that need one.
//!
//! Both the compile checks (D44) and the ABI boundary tests (D45) have
//! to locate a compiler, put its own directory on `PATH` so it can load
//! its runtime DLLs, and fail rather than skip when there is none. That
//! is one policy, so it lives in one place.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;

/// Point this at a compiler to override discovery — a full path, or a
/// command on `PATH`. Its directory is prepended to `PATH` for the run.
pub const CC_OVERRIDE: &str = "REMANENCE_CC";
/// The same, for the C++ compiler used by the `cpp_compat` check.
pub const CXX_OVERRIDE: &str = "REMANENCE_CXX";
/// Set this to skip the checks deliberately. As elsewhere, an unrun
/// check must be somebody's decision rather than a tool's absence.
pub const SKIP: &str = "REMANENCE_SKIP_CC";

/// MSYS2 is a Windows thing, so everything about it is compiled only
/// there: the names, the search, and the advice a failure gives. A
/// message telling a Linux developer to install MSYS2 and set
/// `MSYS_HOME` would be worse than no message.
#[cfg(windows)]
mod msys {
    use std::path::PathBuf;

    /// Names the MSYS2 installation, when it is not where it installs.
    pub const HOME: &str = "MSYS_HOME";
    /// Where MSYS2 installs unless told otherwise.
    pub const DEFAULT: &str = "C:/msys64";

    /// The toolchain directories to try before `PATH`.
    ///
    /// Before, because `PATH` on a developer's Windows box has surprises
    /// on it: a bare `cl` can resolve to **Watcom's**, not MSVC's, which
    /// is why no `cl` is consulted at any point.
    pub fn dirs() -> Vec<PathBuf> {
        let root = std::env::var_os(HOME)
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT));
        vec![root.join("ucrt64/bin"), root.join("mingw64/bin")]
    }

    /// How to fix an absent compiler, in Windows' terms.
    pub fn advice() -> String {
        format!(
            "  - install MSYS2's ucrt64 toolchain (expected at {DEFAULT})\n  \
             - set {HOME}=<msys2 root> if it is installed elsewhere\n"
        )
    }
}

/// Directories searched ahead of `PATH`. Only Windows has any.
pub fn toolchain_dirs() -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        msys::dirs()
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

/// The platform's own advice for installing a compiler.
pub fn toolchain_advice() -> String {
    #[cfg(windows)]
    {
        msys::advice()
    }
    #[cfg(not(windows))]
    {
        "  - install a C toolchain through the system package manager\n".to_owned()
    }
}

pub fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

pub fn scratch() -> PathBuf {
    let dir = crate_dir().join("../../target/c-surface");
    std::fs::create_dir_all(&dir).expect("a scratch directory for object files");
    dir
}

/// A compiler that answered `--version`, and the directory to put on
/// `PATH` so it can find its own runtime DLLs. Without that, MSYS2's
/// `g++` exits non-zero and prints **nothing at all**, which reads as a
/// compile failure with no diagnostic — the confusing failure this
/// function exists to prevent.
pub struct Compiler {
    pub program: PathBuf,
    pub bin_dir: Option<PathBuf>,
}

impl Compiler {
    pub fn command(&self) -> Command {
        let mut command = Command::new(&self.program);
        if let Some(dir) = &self.bin_dir {
            let existing = std::env::var("PATH").unwrap_or_default();
            command.env("PATH", format!("{};{existing}", dir.display()));
        }
        command
    }

    pub fn answers(program: &Path, bin_dir: Option<PathBuf>) -> Option<Self> {
        let candidate = Compiler {
            program: program.to_path_buf(),
            bin_dir,
        };
        let ok = candidate
            .command()
            .arg("--version")
            .output()
            .is_ok_and(|output| output.status.success());
        ok.then_some(candidate)
    }
}

pub fn find(override_var: &str, names: &[&str]) -> Option<Compiler> {
    if let Some(stated) = std::env::var_os(override_var) {
        let path = PathBuf::from(&stated);
        let bin_dir = path.parent().filter(|p| !p.as_os_str().is_empty());
        let found = Compiler::answers(&path, bin_dir.map(Path::to_path_buf));
        assert!(
            found.is_some(),
            "{override_var} is set to {stated:?}, and that does not answer \
             `--version`. Point it at a working compiler or unset it; \
             discovery is not attempted while it is set, so a typo here \
             fails loudly rather than falling back to something else."
        );
        return found;
    }

    for dir in toolchain_dirs() {
        for name in names {
            let path = dir.join(format!("{name}.exe"));
            if path.exists() {
                if let Some(found) = Compiler::answers(&path, Some(dir.clone())) {
                    return Some(found);
                }
            }
        }
    }
    for name in names {
        if let Some(found) = Compiler::answers(Path::new(name), None) {
            return Some(found);
        }
    }
    None
}

pub fn skipping() -> bool {
    if std::env::var_os(SKIP).is_some() {
        eprintln!("!! {SKIP} is set: the C surface was NOT compiled.");
        return true;
    }
    false
}

pub fn require(override_var: &str, names: &[&str], language: &str) -> Option<Compiler> {
    let found = find(override_var, names);
    let searched = toolchain_dirs();
    let where_looked = if searched.is_empty() {
        "on PATH".to_owned()
    } else {
        format!("in {searched:?}, then on PATH")
    };
    assert!(
        found.is_some(),
        "no {language} compiler was found, so the C surface went \
         unchecked. This fails rather than skips, because a check that \
         quietly does not run reads exactly like a check that passed.\n\n\
         Tried {names:?} {where_looked}. A bare `cl` is never tried: it \
         can resolve to Watcom's rather than MSVC's.\n\n\
         Fix it with any of:\n{}  \
         - put a compiler on PATH\n  \
         - set {override_var}=<path to the compiler>\n\n\
         To skip deliberately, set {SKIP}=1.",
        toolchain_advice()
    );
    found
}

pub fn compile(compiler: &Compiler, source: &Path, object: &str, extra: &[&str]) -> Result<(), String> {
    let output = compiler
        .command()
        .args(["-c", "-Wall", "-Wextra", "-Werror"])
        .args(extra)
        .arg("-I")
        .arg(crate_dir().join("include"))
        .arg("-o")
        .arg(scratch().join(object))
        .arg(source)
        .output()
        .map_err(|error| format!("cannot run the compiler: {error}"))?;

    if output.status.success() {
        return Ok(());
    }
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    Err(if text.trim().is_empty() {
        format!(
            "the compiler exited {} and printed nothing, which usually \
             means it could not load its own runtime DLLs — check the \
             directory beside {} is reachable",
            output.status,
            compiler.program.display()
        )
    } else {
        text
    })
}
