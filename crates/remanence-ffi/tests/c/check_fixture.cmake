# SPDX-FileCopyrightText: 2026 Paul Galbraith
# SPDX-License-Identifier: GPL-3.0-only

# A CTest FIXTURES_SETUP step: fails with the fix rather than letting a
# consumer's bare `fopen` report a missing fixture with no hint at all.
#
# Usage: cmake -DFIXTURE=<path> -P check_fixture.cmake

if(NOT DEFINED FIXTURE)
    message(FATAL_ERROR "check_fixture.cmake: FIXTURE was not set")
endif()

if(NOT EXISTS "${FIXTURE}")
    message(FATAL_ERROR
        "the fixture ${FIXTURE} is missing. Run:\n\n"
        "  uv run --directory test-fixture-prep prep_fixtures.py\n")
endif()
