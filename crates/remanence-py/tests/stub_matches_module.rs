// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The type stub says the same thing the module does (S3, D40, D42).
//!
//! `python/src/remanence/__init__.pyi` is written by hand and nothing
//! regenerates it, so it is the one artifact here that can disagree with
//! the surface it describes and stay silent about it. This test is what
//! makes the disagreement loud.
//!
//! **It compares the stub against the module's defining source**, not
//! against a built module: what `src/lib.rs` registers with pyo3 is what
//! Python meets, so the comparison needs no wheel, no interpreter and no
//! environment to install into — which is what a `tests/` target in this
//! cdylib-only crate can actually do. The module is the norm either way:
//! a mismatch is a bug in the stub.
//!
//! **The parsers refuse what they do not understand.** Both sides lean
//! on this file's rustfmt-stable shape — `#[pyclass]`, `#[pymethods]`,
//! `impl` and `pub struct` at column 0, their members indented four —
//! and a construct that does not fit fails the test rather than being
//! skipped. Silent under-detection is the one failure mode that would
//! make this test worse than useless, because it would pass a stub with
//! a hole in it.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

/// Attributes the binding sets on an instance with `setattr`, which no
/// static reading of the source can see as members. Each one is surface
/// and each is declared in the stub; they are listed here so the stub
/// may declare them without the Rust side appearing to lack them.
const INSTANCE_ONLY: &[(&str, &[&str])] = &[("Error", &["category", "rule"])];

/// Names the stub defines for its own use, which are not module
/// attributes: type aliases it refers to in signatures.
const STUB_LOCAL: &[&str] = &["MediaSource"];

fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(relative: &str) -> String {
    let path = crate_dir().join(relative);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
}

// ------------------------------------------------------- the Rust side

#[derive(Default)]
struct Module {
    /// Every `#[pyclass]` type, and the members it exposes.
    classes: BTreeMap<String, BTreeSet<String>>,
    /// Classes with a `#[new]`, which Python constructs directly.
    constructible: BTreeSet<String>,
    /// Every type reaching Python through `add_class`.
    registered: BTreeSet<String>,
    /// Every `#[pyfunction]` reaching Python through `add_function`.
    functions: BTreeSet<String>,
    /// Every name added with `m.add("...", ...)`.
    added: BTreeSet<String>,
}

/// The name a `pub struct`/`pub enum` line declares, at column 0.
fn declared_type(line: &str) -> Option<&str> {
    let rest = line
        .strip_prefix("pub struct ")
        .or_else(|| line.strip_prefix("pub enum "))?;
    Some(
        rest.split(|c: char| !(c.is_alphanumeric() || c == '_'))
            .next()
            .unwrap_or(""),
    )
}

fn parse_module(source: &str) -> Module {
    let lines: Vec<&str> = source.lines().collect();
    let mut module = Module::default();

    assert!(
        !source.contains("pyo3(name"),
        "a `#[pyo3(name = ...)]` remap appeared in the binding. This test \
         reads Rust identifiers as Python names, which that attribute \
         breaks; teach the parser before using it."
    );

    let mut at = 0;
    while at < lines.len() {
        let line = lines[at];

        // A pyclass: its declaration, then whichever fields it exposes.
        if line.starts_with("#[pyclass") {
            assert!(
                line.ends_with(']'),
                "line {}: a `#[pyclass...]` attribute spans several lines, \
                 which this parser does not read. Keep it on one line or \
                 teach the parser.",
                at + 1
            );
            let get_all = line.contains("get_all");
            let mut cursor = at + 1;
            while cursor < lines.len() && declared_type(lines[cursor]).is_none() {
                assert!(
                    lines[cursor].starts_with('#') || lines[cursor].starts_with("pub "),
                    "line {}: `#[pyclass]` on line {} is not followed by a \
                     `pub struct` or `pub enum` at column 0; this parser \
                     cannot tell what it names.",
                    cursor + 1,
                    at + 1
                );
                cursor += 1;
            }
            assert!(
                cursor < lines.len(),
                "unterminated `#[pyclass]` at line {}",
                at + 1
            );
            let name = declared_type(lines[cursor]).unwrap().to_owned();

            let mut members = BTreeSet::new();
            if lines[cursor].ends_with('{') {
                let mut field = cursor + 1;
                let mut tagged = false;
                while field < lines.len() && lines[field] != "}" {
                    let text = lines[field].trim();
                    if text == "#[pyo3(get)]" {
                        tagged = true;
                    } else if let Some(rest) = text.strip_prefix("pub ") {
                        if get_all || tagged {
                            if let Some(field_name) = rest.split(':').next() {
                                members.insert(field_name.trim().to_owned());
                            }
                        }
                        tagged = false;
                    }
                    field += 1;
                }
            }
            module.classes.entry(name).or_default().extend(members);
            at = cursor;
        }
        // A pymethods block: every fn immediately inside it is exposed.
        else if line == "#[pymethods]" {
            let mut cursor = at + 1;
            while cursor < lines.len() && !lines[cursor].starts_with("impl ") {
                cursor += 1;
            }
            assert!(
                cursor < lines.len(),
                "line {}: `#[pymethods]` with no `impl` at column 0 after it",
                at + 1
            );
            let header = lines[cursor];
            let name = header
                .trim_start_matches("impl ")
                .split(|c: char| !(c.is_alphanumeric() || c == '_'))
                .find(|part| !part.is_empty())
                .expect("an impl names a type")
                .to_owned();

            let mut members = BTreeSet::new();
            let mut is_new = false;
            let mut body = cursor + 1;
            while body < lines.len() && lines[body] != "}" {
                let text = lines[body];
                if text == "    #[new]" {
                    is_new = true;
                }
                if let Some(rest) = text
                    .strip_prefix("    pub fn ")
                    .or_else(|| text.strip_prefix("    fn "))
                {
                    let method = rest
                        .split(|c: char| !(c.is_alphanumeric() || c == '_'))
                        .next()
                        .unwrap_or("")
                        .to_owned();
                    if is_new {
                        module.constructible.insert(name.clone());
                        is_new = false;
                    } else if !method.starts_with("__") {
                        // Dunders are compared only through the
                        // constructible check below; `__repr__` and the
                        // context-manager pair are conventional and a
                        // stub that omits one loses nothing.
                        members.insert(method);
                    }
                }
                body += 1;
            }
            module.classes.entry(name).or_default().extend(members);
            at = cursor;
        }
        // The registration block.
        else if let Some(rest) = line.trim().strip_prefix("m.add_class::<") {
            if let Some(name) = rest.split('>').next() {
                module.registered.insert(name.to_owned());
            }
        } else if let Some(rest) = line.trim().strip_prefix("m.add_function(wrap_pyfunction!(") {
            if let Some(name) = rest.split(',').next() {
                module.added_function(name);
            }
        } else if let Some(rest) = line.trim().strip_prefix("m.add(\"") {
            if let Some(name) = rest.split('"').next() {
                module.added.insert(name.to_owned());
            }
        }

        at += 1;
    }

    module
}

impl Module {
    fn added_function(&mut self, name: &str) {
        self.functions.insert(name.trim().to_owned());
    }
}

// ----------------------------------------------------- the Python side

#[derive(Default)]
struct Stub {
    classes: BTreeMap<String, BTreeSet<String>>,
    constructible: BTreeSet<String>,
    functions: BTreeSet<String>,
    names: BTreeSet<String>,
}

/// The identifier a `def`, `class` or annotated-name line declares.
fn python_name(text: &str) -> Option<String> {
    let rest = text
        .strip_prefix("def ")
        .or_else(|| text.strip_prefix("class "))?;
    Some(
        rest.split(|c: char| !(c.is_alphanumeric() || c == '_'))
            .next()
            .unwrap_or("")
            .to_owned(),
    )
}

fn parse_stub(source: &str) -> Stub {
    let mut stub = Stub::default();
    let mut current: Option<String> = None;

    for line in source.lines() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // Column 0 ends any class body.
        if !line.starts_with(' ') {
            current = None;
            if let Some(name) = python_name(line) {
                if line.starts_with("class ") {
                    stub.classes.entry(name.clone()).or_default();
                    current = Some(name);
                } else {
                    stub.functions.insert(name);
                }
            } else if let Some((name, _)) = line.split_once(": ") {
                // A module-level annotated constant, e.g. `__version__: str`.
                if name.chars().all(|c| c.is_alphanumeric() || c == '_') {
                    stub.names.insert(name.to_owned());
                }
            } else if let Some((name, _)) = line.split_once(" = ") {
                // A stub-local type alias.
                if name.chars().all(|c| c.is_alphanumeric() || c == '_') {
                    stub.names.insert(name.to_owned());
                }
            }
            continue;
        }

        let Some(class) = current.clone() else {
            continue;
        };
        let text = line.trim();
        // Only members directly inside the class body, indented four.
        if !line.starts_with("    ") || line.starts_with("     ") {
            continue;
        }
        if let Some(name) = python_name(text) {
            if name == "__init__" {
                stub.constructible.insert(class.clone());
            } else if !name.starts_with("__") {
                stub.classes.entry(class).or_default().insert(name);
            }
        } else if let Some((name, _)) = text.split_once(": ") {
            if name.chars().all(|c| c.is_alphanumeric() || c == '_') && !name.starts_with("__") {
                stub.classes
                    .entry(class)
                    .or_default()
                    .insert(name.to_owned());
            }
        }
    }

    stub
}

// ------------------------------------------------------ the comparison

fn report(
    problems: &mut Vec<String>,
    what: &str,
    module: &BTreeSet<String>,
    stub: &BTreeSet<String>,
) {
    let missing: Vec<_> = module.difference(stub).cloned().collect();
    let extra: Vec<_> = stub.difference(module).cloned().collect();
    if !missing.is_empty() {
        problems.push(format!(
            "{what}: the module has, the stub lacks -> {missing:?}"
        ));
    }
    if !extra.is_empty() {
        problems.push(format!(
            "{what}: the stub has, the module lacks -> {extra:?}"
        ));
    }
}

#[test]
fn the_stub_states_exactly_what_the_module_registers() {
    let module = parse_module(&read("src/lib.rs"));
    let stub = parse_stub(&read("python/src/remanence/__init__.pyi"));

    assert!(
        module.registered.len() > 40,
        "only {} classes were found registered, which means the parser \
         stopped reading rather than that the surface shrank",
        module.registered.len()
    );

    let mut problems = Vec::new();

    // Every registered class, function and added name is in the stub,
    // and the stub invents nothing beyond its own type aliases.
    let mut stub_top: BTreeSet<String> = stub.classes.keys().cloned().collect();
    stub_top.extend(stub.functions.iter().cloned());
    stub_top.extend(stub.names.iter().cloned());
    for local in STUB_LOCAL {
        stub_top.remove(*local);
    }
    stub_top.retain(|name| !name.starts_with('_') || name.starts_with("__"));

    let mut module_top = module.registered.clone();
    module_top.extend(module.functions.iter().cloned());
    module_top.extend(module.added.iter().cloned());
    report(&mut problems, "module surface", &module_top, &stub_top);

    // Every pyclass actually reaches Python.
    for name in module.classes.keys() {
        if !module.registered.contains(name) {
            problems.push(format!(
                "{name} is a #[pyclass] that `add_class` never registers, so \
                 no caller can reach it"
            ));
        }
    }

    // Member-by-member, for the classes both sides agree exist.
    for (name, members) in &module.classes {
        if !module.registered.contains(name) {
            continue;
        }
        let Some(declared) = stub.classes.get(name) else {
            continue; // already reported as a missing class
        };
        let mut allowed = declared.clone();
        for (class, attributes) in INSTANCE_ONLY {
            if class == name {
                for attribute in *attributes {
                    allowed.remove(*attribute);
                }
            }
        }
        report(&mut problems, name, members, &allowed);
    }

    // A class Python can construct is a class the stub gives `__init__`.
    report(
        &mut problems,
        "constructible classes (#[new] vs __init__)",
        &module.constructible,
        &stub.constructible,
    );

    assert!(
        problems.is_empty(),
        "the type stub and the module disagree. The module is the norm, so \
         each line below is a fix to \
         crates/remanence-py/python/src/remanence/__init__.pyi:\n  {}",
        problems.join("\n  ")
    );
}
