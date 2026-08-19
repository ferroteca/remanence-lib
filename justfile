# SPDX-FileCopyrightText: 2026 Paul Galbraith
# SPDX-License-Identifier: GPL-3.0-only

# Drives the C/C++ and Python checks `cargo test` no longer does (D65) —
# neither is triggered by `cargo build`/`cargo test` in any form; both are
# reached only by running the recipes below.
#
# Every recipe is a `#!/usr/bin/env bash` script rather than a bare
# command line, so it runs identically whichever shell `just` itself was
# launched from (PowerShell or a POSIX one) — Git Bash's `bash` is what
# actually executes the body either way, and recipes run with the working
# directory set to this file's own directory (the repository root).

default:
    @just --list

# --- Python (S3) ------------------------------------------------------------

# Builds, stages and tests the Python bindings.
#
# The build is routed through uv (`uv run -- cargo build`) rather than a
# bare `cargo build`, so pyo3 resolves the same interpreter uv will later
# use to run pytest — see AGENTS.md, "uv chooses the interpreter that runs
# the Python suite, always" (D63). A bare `cargo build -p remanence-py`
# from an MSYS2 shell still legitimately builds a module (a58247c); it is
# simply not one this recipe's pytest step could import.
test-py:
    #!/usr/bin/env bash
    set -euo pipefail
    uv run -- cargo build -p remanence-py
    cd crates/remanence-py

    target="../../target/debug"
    stage="$target/py-stage/remanence"
    rm -rf "$stage"
    mkdir -p "$stage"
    if [ -f "$target/remanence_py.dll" ]; then
        cp "$target/remanence_py.dll" "$stage/remanence.pyd"
    elif [ -f "$target/libremanence_py.so" ]; then
        cp "$target/libremanence_py.so" "$stage/remanence.so"
    elif [ -f "$target/libremanence_py.dylib" ]; then
        cp "$target/libremanence_py.dylib" "$stage/remanence.so"
    else
        echo "no compiled Python module in $target" >&2
        exit 1
    fi
    cp python/remanence/__init__.py python/remanence/__init__.pyi python/remanence/py.typed "$stage/"

    stage_root="$(cd "$target/py-stage" && pwd)"
    PYTHONPATH="$stage_root" uv run --with pytest --no-project pytest -q

    export MYPYPATH
    MYPYPATH="$(pwd)/python"
    uv run --with mypy --no-project mypy --strict --python-version 3.10 \
        --no-error-summary --cache-dir ../../target/mypy-cache/accepts \
        tests/mypy_fixtures/accepts.py

    # rejects.py is misuse and must fail to type-check. This only checks
    # that mypy refuses it, not that each line is refused for the exact
    # code its `# expect:` marker names — that finer cross-check lived in
    # the deleted stub_typechecks.rs and is not reproduced here.
    if uv run --with mypy --no-project mypy --strict --python-version 3.10 \
        --no-error-summary --cache-dir ../../target/mypy-cache/rejects \
        tests/mypy_fixtures/rejects.py; then
        echo "rejects.py type-checked clean; it is meant to be refused" >&2
        exit 1
    fi

# --- C / C++ (S2) ------------------------------------------------------------

# Builds the shipped and leak-probe cdylibs (via xtask — see its own doc
# comment for why the toolchain-matching logic lives there and not here),
# configures and builds the CMake C/C++ test project against them, and
# runs every CTest test. Extra arguments pass through to `ctest` — e.g.
# `just test-ffi -LE "rigs|fixtures"` for the set that needs no
# downloaded or generated fixture — ctest's `-LE` takes one regex, not a
# flag per label; passing it twice keeps only the second.
test-ffi *ctest_args:
    #!/usr/bin/env bash
    set -euo pipefail
    out="$(cargo run -q -p xtask)"
    slug="$(printf '%s\n' "$out" | sed -n 's/^SLUG=//p')"
    cmake_args=()
    while IFS= read -r line; do
        [ -n "$line" ] && cmake_args+=("${line#CMAKE_ARG=}")
    done < <(printf '%s\n' "$out" | grep '^CMAKE_ARG=')

    build="target/c-tests-$slug"
    cmake -S crates/remanence-ffi/tests/c -B "$build" \
        -DREMANENCE_INCLUDE="$(pwd)/crates/remanence-ffi/include" \
        -DREMANENCE_EXAMPLES="$(pwd)/crates/remanence-ffi/examples" \
        "${cmake_args[@]}"
    cmake --build "$build" --config Debug

    # `{{ctest_args}}` is `just` inserting raw text, not a properly quoted
    # argv — bare on the `ctest` line below it would let a `|` (as in
    # `-LE "rigs|fixtures"`) be parsed as a shell pipe instead of staying
    # part of the regex. `read -a` splits it on whitespace only, so each
    # token keeps whatever punctuation it was passed with.
    raw_ctest_args="{{ctest_args}}"
    read -ra extra_ctest_args <<< "$raw_ctest_args"
    ctest --test-dir "$build" --build-config Debug --output-on-failure "${extra_ctest_args[@]}"
