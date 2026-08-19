# SPDX-FileCopyrightText: 2026 Paul Galbraith
# SPDX-License-Identifier: GPL-3.0-only

"""The stub describes the module it ships beside (S3, D40, D48).

`stub_matches_module.rs` asks the same question of the *source*, because
a Rust test has no built module to import. This one asks it of the
**installed module**, which is both stronger and the only form that means
anything in an sdist a stranger unpacked: it compares what the stub
declares against what `import remanence` actually provides.

It reads the stub from beside the module rather than from the repository,
so it verifies the artifact rather than the tree it was built from.
"""

import ast
import inspect
import pathlib

import pytest

import remanence

#: Names the stub defines for its own use in signatures, which are not
#: module attributes.
STUB_LOCAL = {"MediaSource"}

#: Set on every instance by the binding rather than declared on the
#: class, so no introspection of the class can see them.
INSTANCE_ONLY = {"Error": {"category", "rule"}}


@pytest.fixture(scope="module")
def stub():
    """The stub as shipped, beside the module it describes."""
    path = pathlib.Path(remanence.__file__).with_name("__init__.pyi")
    if not path.exists():
        pytest.fail(
            f"no type stub beside the installed module (looked in "
            f"{path.parent}). It ships in both the wheel and the sdist; "
            f"its absence is a packaging regression, not a missing test "
            f"dependency."
        )
    return ast.parse(path.read_text(encoding="utf-8"))


def _stub_classes(tree):
    return {
        node.name: {
            member.name
            for member in node.body
            if isinstance(member, ast.FunctionDef)
            and not member.name.startswith("_")
        }
        | {
            member.target.id
            for member in node.body
            if isinstance(member, ast.AnnAssign)
            and isinstance(member.target, ast.Name)
            and not member.target.id.startswith("_")
        }
        for node in tree.body
        if isinstance(node, ast.ClassDef)
    }


def _stub_top_level(tree):
    names = set()
    for node in tree.body:
        if isinstance(node, (ast.ClassDef, ast.FunctionDef)):
            names.add(node.name)
        elif isinstance(node, ast.AnnAssign) and isinstance(node.target, ast.Name):
            names.add(node.target.id)
        elif isinstance(node, ast.Assign):
            names.update(
                target.id for target in node.targets if isinstance(target, ast.Name)
            )
    return {name for name in names if not name.startswith("_")} - STUB_LOCAL


def _module_top_level():
    # `remanence` is bound by the package shim's star-import: the
    # extension submodule, not part of the typed surface.
    return {
        name
        for name in dir(remanence)
        if not name.startswith("_") and name != "remanence"
    }


def test_the_marker_that_makes_the_stub_count_is_present():
    marker = pathlib.Path(remanence.__file__).with_name("py.typed")
    assert marker.exists(), (
        "py.typed is missing, so a type checker ignores the stub entirely "
        "(PEP 561) however correct it is"
    )


def test_every_name_the_module_exports_is_declared(stub):
    missing = _module_top_level() - _stub_top_level(stub)
    assert not missing, f"the module exports these and the stub lacks them: {sorted(missing)}"


def test_the_stub_invents_nothing(stub):
    extra = _stub_top_level(stub) - _module_top_level()
    assert not extra, f"the stub declares these and the module lacks them: {sorted(extra)}"


def test_every_class_member_agrees(stub):
    declared = _stub_classes(stub)
    problems = []

    for name, members in declared.items():
        obj = getattr(remanence, name, None)
        if obj is None or not inspect.isclass(obj):
            continue

        inherited = set()
        for base in obj.__mro__[1:]:
            inherited |= {member for member in dir(base) if not member.startswith("_")}
        real = {
            member for member in dir(obj) if not member.startswith("_")
        } - inherited

        allowed = members - INSTANCE_ONLY.get(name, set())
        if real - members:
            problems.append(f"{name}: module has, stub lacks -> {sorted(real - members)}")
        if allowed - real:
            problems.append(f"{name}: stub has, module lacks -> {sorted(allowed - real)}")

    assert not problems, "the stub and the module disagree:\n  " + "\n  ".join(problems)


def test_the_exception_carries_the_attributes_the_stub_promises():
    """The two the class cannot show, asserted on a real instance."""
    with pytest.raises(remanence.Error) as refusal:
        remanence.discover_media("no-such-artifact-anywhere.img", writable=False)
    error = refusal.value
    assert isinstance(error.category, str)
    assert error.rule is None or isinstance(error.rule, str)
