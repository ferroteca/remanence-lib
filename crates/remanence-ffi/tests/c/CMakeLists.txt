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
