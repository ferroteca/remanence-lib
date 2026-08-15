// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The type stub's *types* are usable, and still refuse misuse (S3, D43).
//!
//! `stub_matches_module.rs` checks names — that the stub declares what
//! the module registers, member for member. It cannot check types: it
//! reads text, and whether `rule` is `str | None` is not a question text
//! comparison answers. This test is the other half.
//!
//! It runs `mypy --strict` **against the stub source**, not against a
//! built wheel, which needs no interpreter of the right version, no
//! build and no environment to install into — and is strictly better
//! besides: mypy reports errors *inside* a first-party stub, where a
//! stub reached through an installed package is followed silently. That
//! is not a hypothetical improvement; it is how the shadowed `bytes`
//! members on `File` and `LocationBytes` were found.
//!
//! Two fixtures, and the second is the one that matters:
//!
//! - `mypy_fixtures/accepts.py` — ordinary consumer code, which must check
//!   clean. It fails when a declared type is wrong or unusable.
//! - `mypy_fixtures/rejects.py` — misuse, which must be refused, each line
//!   naming the mypy error code it expects. A stub that has quietly
//!   degraded to `Any` still lets `accepts.py` pass; it stops refusing
//!   what is wrong, and only this fixture notices.
//!
//! The check runs at **Python 3.10**, the minimum `pyproject.toml`
//! claims, so the stub is verified against the oldest version the
//! distribution promises to serve rather than whichever one is at hand.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Set this to skip the check deliberately. It is deliberately awkward
/// and deliberately named: an unrun check must be somebody's decision,
/// never the quiet result of a tool being absent.
const SKIP: &str = "REMANENCE_SKIP_MYPY";

fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The ways mypy might be reachable, cheapest first. The last needs no
/// prior install, which is why it is there at all.
fn candidates() -> Vec<(&'static str, Vec<&'static str>)> {
    vec![
        ("python -m mypy", vec!["python", "-m", "mypy"]),
        ("mypy", vec!["mypy"]),
        (
            "uv run --with mypy",
            vec!["uv", "run", "--with", "mypy", "--no-project", "mypy"],
        ),
    ]
}

fn mypy_command() -> Option<(&'static str, Vec<&'static str>)> {
    for (label, argv) in candidates() {
        let mut probe = Command::new(argv[0]);
        probe.args(&argv[1..]).arg("--version");
        if let Ok(output) = probe.output() {
            if output.status.success() {
                return Some((label, argv));
            }
        }
    }
    None
}

struct Run {
    status_ok: bool,
    output: String,
}

fn run_mypy(argv: &[&str], fixture: &Path) -> Run {
    let target = crate_dir()
        .join("../../target/mypy-cache")
        .join(fixture.file_stem().expect("fixture has a name"));

    let output = Command::new(argv[0])
        .args(&argv[1..])
        .arg("--strict")
        .arg("--python-version")
        .arg("3.10")
        .arg("--no-error-summary")
        .arg("--cache-dir")
        .arg(&target)
        .arg(fixture)
        .env("MYPYPATH", crate_dir().join("python"))
        .current_dir(crate_dir())
        .output()
        .unwrap_or_else(|error| panic!("cannot run mypy: {error}"));

    Run {
        status_ok: output.status.success(),
        output: format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ),
    }
}

/// `path:LINE: error: message  [code]` -> line number and code. Notes
/// and anything else are not errors and are ignored.
fn errors_by_line(output: &str) -> BTreeMap<usize, Vec<String>> {
    let mut found: BTreeMap<usize, Vec<String>> = BTreeMap::new();
    for line in output.lines() {
        let Some((location, rest)) = line.split_once(": error: ") else {
            continue;
        };
        // `path:LINE` normally, `path:LINE:COLUMN` if columns are on;
        // a Windows drive letter puts another colon in the path, so take
        // the last segment that is a number rather than a fixed position.
        let Some(number) = location
            .rsplit(':')
            .find_map(|part| part.trim().parse::<usize>().ok())
        else {
            continue;
        };
        let code = rest
            .rsplit_once('[')
            .and_then(|(_, tail)| tail.strip_suffix(']'))
            .unwrap_or("no-code")
            .to_owned();
        found.entry(number).or_default().push(code);
    }
    found
}

/// The `# expect: <code>` markers a fixture carries, by line.
fn expectations(source: &str) -> BTreeMap<usize, String> {
    source
        .lines()
        .enumerate()
        .filter_map(|(index, line)| {
            let (_, marker) = line.split_once("# expect: ")?;
            Some((index + 1, marker.trim().to_owned()))
        })
        .collect()
}

fn require_mypy() -> Option<(&'static str, Vec<&'static str>)> {
    if std::env::var_os(SKIP).is_some() {
        eprintln!("!! {SKIP} is set: the stub's types were NOT checked.");
        return None;
    }
    let found = mypy_command();
    assert!(
        found.is_some(),
        "mypy is not reachable, so the stub's types went unchecked. This \
         fails rather than skips, because a check that quietly does not \
         run reads exactly like a check that passed.\n\n\
         Any one of these fixes it:\n  \
         - install uv (the project already uses it to build the wheel), \
         and this test fetches mypy itself\n  \
         - pip install mypy into the interpreter on PATH\n\n\
         To skip deliberately, set {SKIP}=1."
    );
    found
}

#[test]
fn a_consumer_type_checks_against_the_stub() {
    let Some((label, argv)) = require_mypy() else {
        return;
    };
    let fixture = crate_dir().join("tests/mypy_fixtures/accepts.py");
    let run = run_mypy(&argv, &fixture);
    assert!(
        run.status_ok,
        "ordinary consumer code does not type-check against the stub \
         (mypy via {label}). Each line below is a fix to \
         crates/remanence-py/python/remanence/__init__.pyi, the module \
         being the norm:\n{}",
        run.output
    );
}

#[test]
fn the_stub_still_refuses_misuse() {
    let Some((label, argv)) = require_mypy() else {
        return;
    };
    let path = crate_dir().join("tests/mypy_fixtures/rejects.py");
    let source = std::fs::read_to_string(&path).expect("the rejects fixture is readable");
    let expected = expectations(&source);
    assert!(
        expected.len() >= 8,
        "only {} expectations were found in rejects.py, which means the \
         markers stopped being read rather than that misuse stopped being \
         possible",
        expected.len()
    );

    let run = run_mypy(&argv, &path);
    assert!(
        !run.status_ok,
        "rejects.py type-checked clean (mypy via {label}). Every line in it \
         is meant to be refused, so the stub has stopped refusing \
         misuse — most likely it has degraded to `Any`, through a lost \
         py.typed, a widened parameter, or a class the checker no longer \
         resolves.\n{}",
        run.output
    );

    let found = errors_by_line(&run.output);
    let mut problems = Vec::new();

    for (line, code) in &expected {
        match found.get(line) {
            None => problems.push(format!(
                "line {line}: expected a `{code}` error and mypy reported none"
            )),
            Some(codes) if !codes.contains(code) => problems.push(format!(
                "line {line}: expected `{code}`, mypy reported {codes:?}"
            )),
            Some(_) => {}
        }
    }
    for (line, codes) in &found {
        if !expected.contains_key(line) {
            problems.push(format!(
                "line {line}: mypy reported {codes:?}, which no `# expect:` \
                 marker asked for — add the marker or fix the fixture"
            ));
        }
    }

    assert!(
        problems.is_empty(),
        "rejects.py did not fail the way it says it should:\n  {}\n\nmypy \
         output was:\n{}",
        problems.join("\n  "),
        run.output
    );
}
