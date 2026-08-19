# SPDX-FileCopyrightText: 2026 Paul Galbraith
# SPDX-License-Identifier: GPL-3.0-only

# The C surface's tests, in C and in C++ (S2, D44, D45, D46, D53).
#
# CMake is here for one reason: MSVC. `cl.exe` needs the environment
# `vcvars64.bat` sets, and locating and sourcing that from a test harness
# is more bespoke machinery than the gcc discovery it replaces. CMake
# does it, and finds the compiler unaided. Everything else follows from
# that — the toolchain search, the generator, the warning flags per
# compiler, and the C/C++ split.
#
# The compiler is the library's, not this file's. Where the cdylib is
# MSVC-built the search above is the native match; where a windows-gnu
# rustc built it instead, the harness names gcc, because MSVC cannot
# link a MinGW import library and the two disagree about the C runtime
# besides. Either way what arrives here is a compiler already chosen.
#
# `just test-ffi` drives this (D65): building the shipped and leak-probe
# cdylibs, configuring, building, and running `ctest`. It is not meant to
# be configured by hand, though nothing stops it — it needs REMANENCE_LIB
# pointing at the built import library or shared object, and
# REMANENCE_PROBE_LIB/REMANENCE_RUNTIME/REMANENCE_PROBE_RUNTIME are
# optional beyond that (see the CTest registration below for what each
# test needs).

cmake_minimum_required(VERSION 3.20)
project(remanence_c_tests C CXX)

set(CMAKE_C_STANDARD 11)
set(CMAKE_C_STANDARD_REQUIRED ON)
set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

if(NOT DEFINED REMANENCE_LIB)
    message(FATAL_ERROR
        "REMANENCE_LIB is not set. It must point at the built library: "
        "target/<profile>/remanence_ffi.dll.lib on MSVC, "
        "libremanence_ffi.dll.a on a MinGW toolchain, or the .so/.dylib "
        "elsewhere. `cargo build` produces it; `cargo test` does not.")
endif()

if(NOT DEFINED REMANENCE_INCLUDE)
    message(FATAL_ERROR "REMANENCE_INCLUDE is not set.")
endif()

if(NOT DEFINED REMANENCE_EXAMPLES)
    message(FATAL_ERROR "REMANENCE_EXAMPLES is not set.")
endif()

# One predictable output directory whichever generator is in use.
# Multi-config generators (Visual Studio) otherwise bury binaries under
# a per-config subdirectory the caller would have to guess.
foreach(suffix "" _DEBUG _RELEASE _RELWITHDEBINFO _MINSIZEREL)
    set(CMAKE_RUNTIME_OUTPUT_DIRECTORY${suffix} "${CMAKE_BINARY_DIR}/bin")
endforeach()

# Warnings are errors. The surface is generated and the callers are
# small; anything a compiler objects to here is a real finding.
if(MSVC)
    add_compile_options(/W4 /WX)
else()
    add_compile_options(-Wall -Wextra -Werror)
endif()

include_directories("${REMANENCE_INCLUDE}")

# --- the header stands alone -------------------------------------------
#
# A translation unit that includes the header and nothing else. Written
# here rather than checked in: its whole content is what it omits, and a
# file in the tree invites someone to add an include to it.
set(selfcontained_c "${CMAKE_BINARY_DIR}/generated/selfcontained.c")
file(WRITE "${selfcontained_c}"
     "#include <remanence.h>\n\nint main(void)\n{\n    return 0;\n}\n")
add_library(header_selfcontained OBJECT "${selfcontained_c}")

# --- the header is valid C++ -------------------------------------------
#
# `cpp_compat = true` in cbindgen.toml claims this. Nothing tested it
# before D44.
set(selfcontained_cpp "${CMAKE_BINARY_DIR}/generated/selfcontained.cpp")
file(WRITE "${selfcontained_cpp}"
     "#include <remanence.h>\n\nint main()\n{\n    return 0;\n}\n")
add_library(header_cpp OBJECT "${selfcontained_cpp}")

# --- the C++ header stands alone too -----------------------------------
#
# `remanence.hpp` is the derived C++ presentation of the same ABI (D53).
# It is hand-maintained rather than generated, so unlike the C header it
# *can* fall behind the Rust — which is what makes compiling it a check
# rather than a formality. Written here for the same reason as the two
# above: its whole content is what it omits.
set(selfcontained_hpp "${CMAKE_BINARY_DIR}/generated/selfcontained_hpp.cpp")
file(WRITE "${selfcontained_hpp}"
     "#include <remanence.hpp>\n\nint main()\n{\n    return 0;\n}\n")
add_library(header_hpp OBJECT "${selfcontained_hpp}")

# --- the dangling one-liner does not compile ---------------------------
#
# The header deletes every borrowed-record accessor on an rvalue, so
# `medium.identify().layers()` — views of a handle that dies at the
# semicolon — is a compile error rather than a dangling read (D53). A
# claim that something is *refused* is only worth as much as a check
# that it still is, so this compiles it and fails when it succeeds.
#
# **Compiled, not linked.** A `try_compile` builds an executable by
# default, which needs the library these snippets never link against —
# so both would "fail" for a reason that has nothing to do with what
# they claim, and the refusal would read as a pass. A static library
# stops at the object file, which is where the answer is.
set(CMAKE_TRY_COMPILE_TARGET_TYPE STATIC_LIBRARY)

set(dangling_hpp "${CMAKE_BINARY_DIR}/generated/dangling_hpp.cpp")
file(WRITE "${dangling_hpp}"
     "#include <remanence.hpp>\n\n"
     "std::size_t count(remanence::Medium medium)\n"
     "{\n"
     "    return medium.identify().layers().size();\n"
     "}\n")
try_compile(dangling_compiled "${CMAKE_BINARY_DIR}/generated/dangling"
    SOURCES "${dangling_hpp}"
    CMAKE_FLAGS "-DINCLUDE_DIRECTORIES=${REMANENCE_INCLUDE}"
    CXX_STANDARD 17
    CXX_STANDARD_REQUIRED ON
    OUTPUT_VARIABLE dangling_output)
if(dangling_compiled)
    message(FATAL_ERROR
        "a borrowed record taken from a temporary handle compiled: "
        "remanence.hpp is supposed to delete those overloads, and a "
        "caller writing `medium.identify().layers()` now reads freed "
        "memory instead of meeting a compiler error.")
endif()

# The control, without which the check above is worthless: the same walk
# over a handle bound to a name, which must compile. A wrong include
# path, an unusable standard, or a header that does not parse would
# otherwise fail the file above for a reason that has nothing to do with
# what it claims — and read as a pass.
set(bound_hpp "${CMAKE_BINARY_DIR}/generated/bound_hpp.cpp")
file(WRITE "${bound_hpp}"
     "#include <remanence.hpp>\n\n"
     "std::size_t count(remanence::Medium medium)\n"
     "{\n"
     "    remanence::Identification what = medium.identify();\n"
     "    return what.layers().size();\n"
     "}\n")
try_compile(bound_compiled "${CMAKE_BINARY_DIR}/generated/bound"
    SOURCES "${bound_hpp}"
    CMAKE_FLAGS "-DINCLUDE_DIRECTORIES=${REMANENCE_INCLUDE}"
    CXX_STANDARD 17
    CXX_STANDARD_REQUIRED ON
    OUTPUT_VARIABLE bound_output)
if(NOT bound_compiled)
    message(FATAL_ERROR
        "the same walk over a named handle did not compile, so the "
        "refusal checked above proves nothing about temporaries:\n"
        "${bound_output}")
endif()

unset(CMAKE_TRY_COMPILE_TARGET_TYPE)

# --- the shipped examples still compile --------------------------------
add_library(example_identify OBJECT "${REMANENCE_EXAMPLES}/identify.c")
add_library(example_identify_cpp OBJECT "${REMANENCE_EXAMPLES}/identify.cpp")

# --- a C caller across the boundary ------------------------------------
add_executable(abi_boundary abi_boundary.c)
target_link_libraries(abi_boundary PRIVATE "${REMANENCE_LIB}")

# --- a C++ caller through the wrapper ----------------------------------
#
# The same boundary one layer out: what this exercises is the header's
# own claims — typed refusals, RAII lifetimes, absences that stay
# absences — which only running a C++ caller can show.
add_executable(wrapper wrapper.cpp)
target_link_libraries(wrapper PRIVATE "${REMANENCE_LIB}")

# --- the leak probe ----------------------------------------------------
#
# This one links a **different build** of the library: the probe is a
# global allocator and an exported symbol, so it never reaches the
# shipped cdylib, and the Rust harness builds it into its own target
# directory (D50). Its own output directory too — both builds carry the
# same file name, and Windows loads the copy beside the executable.
#
# `wrapper_leaks` is the same measurement for the C++ presentation, and
# the one that matters most there: a caller who never writes a `_free`
# cannot see a missing one either (D53).
if(REMANENCE_PROBE_LIB)
    add_executable(abi_leaks abi_leaks.c)
    target_link_libraries(abi_leaks PRIVATE "${REMANENCE_PROBE_LIB}")
    add_executable(wrapper_leaks wrapper_leaks.cpp)
    target_link_libraries(wrapper_leaks PRIVATE "${REMANENCE_PROBE_LIB}")
    foreach(suffix "" _DEBUG _RELEASE _RELWITHDEBINFO _MINSIZEREL)
        set_target_properties(abi_leaks wrapper_leaks PROPERTIES
            RUNTIME_OUTPUT_DIRECTORY${suffix} "${CMAKE_BINARY_DIR}/bin/leak")
    endforeach()
endif()

# === CTest registration (D65) ==============================================
#
# `ctest` runs every group below directly. Naming a target (`abi_boundary`,
# `wrapper`, ...) as a test's `COMMAND` resolves to that target's actual
# built executable automatically — the per-config directory, the `.exe`
# suffix — which is what replaced the Rust harness's own path-joining and
# existence assertion.

enable_testing()

get_filename_component(REMANENCE_WORKSPACE
    "${CMAKE_CURRENT_SOURCE_DIR}/../../../.." ABSOLUTE)

# --- runtime DLL staging ---------------------------------------------------
#
# Windows resolves an import from beside the executable before anything on
# PATH. Folded into the build itself, rather than a shell step after, so a
# bare `cmake --build` alone produces runnable executables.
if(WIN32 AND REMANENCE_RUNTIME)
    foreach(target abi_boundary wrapper)
        add_custom_command(TARGET ${target} POST_BUILD
            COMMAND ${CMAKE_COMMAND} -E copy_if_different
                "${REMANENCE_RUNTIME}" "$<TARGET_FILE_DIR:${target}>")
    endforeach()
endif()
if(WIN32 AND REMANENCE_PROBE_LIB AND REMANENCE_PROBE_RUNTIME)
    foreach(target abi_leaks wrapper_leaks)
        add_custom_command(TARGET ${target} POST_BUILD
            COMMAND ${CMAKE_COMMAND} -E copy_if_different
                "${REMANENCE_PROBE_RUNTIME}" "$<TARGET_FILE_DIR:${target}>")
    endforeach()
endif()

# --- fixture existence checks (FIXTURES_SETUP) -----------------------------
#
# A missing fixture otherwise fails a bare `fopen` inside the C/C++ caller
# with no hint at all; this reports the fix instead, and runs once as a
# setup step so every consumer gets it for free. **The setup test carries
# the same LABELS as its consumers** — otherwise excluding a label
# (`ctest -LE rigs`) filters the consumers but not this standalone check,
# which then fails on its own on a machine that deliberately has no
# fixtures.
#
# **Converged onto one label each, which `abi_leaks` and `wrapper report`
# did not carry before this file existed** — both need
# `freedos-parttest.qcow2` exactly as `abi_boundary discovery` does, but
# only the latter was gated (behind Cargo's `rigs` feature). A bare
# `cargo test -p remanence-ffi` on a fresh clone therefore used to fail on
# two undeclared fixture dependencies, the same class of bug D58's history
# (`media_sources`, `sevenzip_catalog`) is about. All three now share the
# `rigs` label and fixture, closing that gap.
set(REMANENCE_FIXTURE_FREEDOS
    "${REMANENCE_WORKSPACE}/crates/remanence/tests/fixtures/freedos-parttest.qcow2")
add_test(NAME fixture_freedos_qcow2
    COMMAND ${CMAKE_COMMAND} "-DFIXTURE=${REMANENCE_FIXTURE_FREEDOS}"
        -P "${CMAKE_CURRENT_SOURCE_DIR}/check_fixture.cmake")
set_tests_properties(fixture_freedos_qcow2 PROPERTIES
    FIXTURES_SETUP "freedos_qcow2" LABELS "rigs")

set(REMANENCE_FIXTURE_PINBALL
    "${REMANENCE_WORKSPACE}/crates/remanence/tests/fixtures/Bill Budge Pinball Construction Set [Commodore 64] (1of2).7z")
add_test(NAME fixture_pinball_capture
    COMMAND ${CMAKE_COMMAND} "-DFIXTURE=${REMANENCE_FIXTURE_PINBALL}"
        -P "${CMAKE_CURRENT_SOURCE_DIR}/check_fixture.cmake")
set_tests_properties(fixture_pinball_capture PROPERTIES
    FIXTURES_SETUP "pinball_capture" LABELS "fixtures")

# --- scratch directories (FIXTURES_SETUP / FIXTURES_CLEANUP) --------------
#
# `wrapper flux` and `wrapper_leaks` each lay a worked example somewhere of
# their own; CTest creates it before and removes it after regardless of
# which test selection (`-R`/`-L`) was asked for, since a required fixture
# is pulled in even when its name was not.
set(REMANENCE_SCRATCH_FLUX "${CMAKE_BINARY_DIR}/scratch/flux")
add_test(NAME scratch_flux_setup
    COMMAND ${CMAKE_COMMAND} -E make_directory "${REMANENCE_SCRATCH_FLUX}")
set_tests_properties(scratch_flux_setup PROPERTIES FIXTURES_SETUP "scratch_flux")
add_test(NAME scratch_flux_cleanup
    COMMAND ${CMAKE_COMMAND} -E remove_directory "${REMANENCE_SCRATCH_FLUX}")
set_tests_properties(scratch_flux_cleanup PROPERTIES FIXTURES_CLEANUP "scratch_flux")

set(REMANENCE_SCRATCH_LEAK "${CMAKE_BINARY_DIR}/scratch/leak")
add_test(NAME scratch_leak_setup
    COMMAND ${CMAKE_COMMAND} -E make_directory "${REMANENCE_SCRATCH_LEAK}")
set_tests_properties(scratch_leak_setup PROPERTIES FIXTURES_SETUP "scratch_leak")
add_test(NAME scratch_leak_cleanup
    COMMAND ${CMAKE_COMMAND} -E remove_directory "${REMANENCE_SCRATCH_LEAK}")
set_tests_properties(scratch_leak_cleanup PROPERTIES FIXTURES_CLEANUP "scratch_leak")

# --- abi_boundary: the C ABI, crossed from C (D45, D46) --------------------
foreach(group catalogs version refusal nulls devices)
    add_test(NAME abi_boundary_${group} COMMAND abi_boundary ${group}
        WORKING_DIRECTORY "${REMANENCE_WORKSPACE}")
endforeach()

add_test(NAME abi_boundary_discovery
    COMMAND abi_boundary discovery
        crates/remanence/tests/fixtures/freedos-parttest.qcow2
    WORKING_DIRECTORY "${REMANENCE_WORKSPACE}")
set_tests_properties(abi_boundary_discovery PROPERTIES
    LABELS "rigs" FIXTURES_REQUIRED "freedos_qcow2")

# --- wrapper: the C++ presentation, crossed from C++ (D53) -----------------
foreach(group catalogs refusals authorship devices views lifetimes absences flux_refusals)
    add_test(NAME wrapper_${group} COMMAND wrapper ${group}
        WORKING_DIRECTORY "${REMANENCE_WORKSPACE}")
endforeach()

add_test(NAME wrapper_flux
    COMMAND wrapper flux "${REMANENCE_SCRATCH_FLUX}"
    WORKING_DIRECTORY "${REMANENCE_WORKSPACE}")
set_tests_properties(wrapper_flux PROPERTIES FIXTURES_REQUIRED "scratch_flux")

add_test(NAME wrapper_report
    COMMAND wrapper report
        crates/remanence/tests/fixtures/freedos-parttest.qcow2
    WORKING_DIRECTORY "${REMANENCE_WORKSPACE}")
set_tests_properties(wrapper_report PROPERTIES
    LABELS "rigs" FIXTURES_REQUIRED "freedos_qcow2")

add_test(NAME wrapper_flux_capture
    COMMAND wrapper flux_capture
        "crates/remanence/tests/fixtures/Bill Budge Pinball Construction Set [Commodore 64] (1of2).7z"
    WORKING_DIRECTORY "${REMANENCE_WORKSPACE}")
set_tests_properties(wrapper_flux_capture PROPERTIES
    LABELS "fixtures" FIXTURES_REQUIRED "pinball_capture")

# --- the leak probe (D47, D50) ---------------------------------------------
#
# Only registered when a probe build was actually linked — REMANENCE_PROBE_LIB
# is optional, unlike REMANENCE_LIB.
if(REMANENCE_PROBE_LIB)
    add_test(NAME abi_leaks
        COMMAND abi_leaks
            crates/remanence/tests/fixtures/freedos-parttest.qcow2
        WORKING_DIRECTORY "${REMANENCE_WORKSPACE}")
    set_tests_properties(abi_leaks PROPERTIES
        LABELS "rigs" FIXTURES_REQUIRED "freedos_qcow2")

    add_test(NAME wrapper_leaks
        COMMAND wrapper_leaks "${REMANENCE_SCRATCH_LEAK}"
        WORKING_DIRECTORY "${REMANENCE_WORKSPACE}")
    set_tests_properties(wrapper_leaks PROPERTIES FIXTURES_REQUIRED "scratch_leak")
endif()

# --- what needs no add_test at all -----------------------------------------
#
# The five `header_*`/`example_*` OBJECT libraries above compile only,
# nothing to run; and the `dangling`/`bound` try_compile checks already ran
# at configure time, above, failing this whole configure via
# `message(FATAL_ERROR ...)` on the wrong outcome. `cmake --build` failing
# outright already surfaces every one of these — there is nothing left for
# `ctest` to wrap.
