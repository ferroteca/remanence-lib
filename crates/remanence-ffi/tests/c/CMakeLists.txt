# SPDX-FileCopyrightText: 2026 Paul Galbraith
# SPDX-License-Identifier: GPL-3.0-only

# The C surface's tests (S2, D44, D45, D46).
#
# CMake is here for one reason: MSVC. `cl.exe` needs the environment
# `vcvars64.bat` sets, and locating and sourcing that from a test harness
# is more bespoke machinery than the gcc discovery it replaces. CMake
# does it, and finds the compiler unaided. Everything else follows from
# that — the toolchain search, the generator, the warning flags per
# compiler, and the C/C++ split.
#
# The Rust tests in `crates/remanence-ffi/tests/` drive this. It is not
# meant to be configured by hand, though nothing stops it: it needs
# REMANENCE_LIB pointing at the built import library or shared object.

cmake_minimum_required(VERSION 3.20)
project(remanence_c_tests C CXX)

set(CMAKE_C_STANDARD 11)
set(CMAKE_C_STANDARD_REQUIRED ON)
set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

if(NOT DEFINED REMANENCE_LIB)
    message(FATAL_ERROR
        "REMANENCE_LIB is not set. It must point at the built library: "
        "target/<profile>/remanence_ffi.dll.lib on MSVC, or the .so/.dylib "
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

# --- the shipped example still compiles --------------------------------
add_library(example_identify OBJECT "${REMANENCE_EXAMPLES}/identify.c")

# --- a C caller across the boundary ------------------------------------
add_executable(abi_boundary abi_boundary.c)
target_link_libraries(abi_boundary PRIVATE "${REMANENCE_LIB}")

# --- the leak probe, when the library was built with it ----------------
#
# Optional because the symbol it calls exists only under the cdylib's
# `leak-probe` feature, which is off for a released artifact. Building
# this target against a library without it is a link error, so it is not
# built unless the caller says the feature is on.
if(REMANENCE_LEAK_PROBE)
    add_executable(abi_leaks abi_leaks.c)
    target_link_libraries(abi_leaks PRIVATE "${REMANENCE_LIB}")
endif()
