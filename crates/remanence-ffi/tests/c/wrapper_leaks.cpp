/*
 * SPDX-FileCopyrightText: 2026 Paul Galbraith
 * SPDX-License-Identifier: GPL-3.0-only
 */

/*
 * Every C++ wrapper gives back what its constructor took (S2, D47, D53).
 *
 * `abi_leaks.c` asserts the `_free` discipline for a C caller who calls
 * the frees by hand. This asserts it for a C++ caller who never calls
 * one: the whole claim of `<remanence.hpp>` is that a destructor
 * discharges the handle, that a moved-from wrapper frees nothing, and
 * that a refusal's message and rule are freed on the way into the
 * exception. Those are exactly the mistakes RAII hides — a leak here
 * would be invisible at the call site, because there is no call site.
 *
 * It is the same measurement and for the same reason: the allocations
 * are Rust's, made inside the cdylib, so no leak checker on this side
 * ever sees them. The library counts its own live blocks and exports the
 * count under its `leak-probe` feature, which never ships. The symbol is
 * declared here rather than included, being no part of S2.
 *
 * **The first cycle is a warm-up whose count is discarded**, because a
 * library settles lazily-initialised state on first use and that is
 * allocation which is never freed and never should be. A leak is the
 * count rising per cycle after that.
 *
 * It fetches nothing: three cycles make their own medium through the
 * authorship door, and the fourth lays the remanence format's own worked
 * example on disk and climbs the flux ladder over it.
 */

#include <remanence.hpp>

#include "worked_example.hpp"

#include <cstdint>
#include <cstdio>
#include <optional>
#include <string>
#include <utility>
#include <vector>

/* Exported by the cdylib only under its `leak-probe` feature. */
extern "C" std::int64_t remanence_probe_live_allocations(void);

namespace {

/* Enough cycles that a one-block-per-cycle leak is unmistakable. */
constexpr int CYCLES = 8;

int failures = 0;

/// A session, a medium, and every owned record they hand out — all
/// released by scope exit alone.
void handles_cycle()
{
    remanence::Session session;
    remanence::Medium blank = session.new_media("chs-disk", 8, 2, 9, 512);

    remanence::Geometry geometry = blank.geometry();
    (void)geometry.coordinates();
    (void)geometry.readings();

    remanence::Assurance assurance = blank.assurance();
    (void)assurance.evidence();

    remanence::Identification identification = blank.identify();
    (void)identification.layers();
    (void)identification.evidence();

    std::optional<remanence::Partition> direct = blank.partition(0);
    if (direct.has_value()) {
        (void)direct->evidence();
        remanence::Volume volume = direct->volume();
        (void)volume.read_at(0, 512);
    }

    // A handle moved from must not free twice, and the one moved to
    // must still free once.
    remanence::Geometry moved = blank.geometry();
    remanence::Geometry landed = std::move(moved);
    (void)landed.state();

    session.release_media(blank.id());
}

/// A refusal, whose message and rule the caller owns — freed on the way
/// into the exception rather than left for the caller to remember.
void refusal_cycle()
{
    try {
        remanence::discover_media("no-such-artifact-anywhere.img");
    } catch (const remanence::Error&) {
        // The refusal is the point; the exception's own copies die here.
    }

    remanence::Session session;
    try {
        session.new_media("no-such-kind-at-all");
    } catch (const remanence::Error&) {
    }
}

/// A handle handed back to C, which frees it — the escape hatch for the
/// functions this header does not wrap.
void released_cycle()
{
    remanence::Session session;
    remanence::Medium blank = session.new_media("chs-disk", 4, 1, 8, 256);

    RemanenceGeometry* raw = blank.geometry().release();
    remanence_geometry_free(raw);

    RemanenceSession* session_raw = session.release();
    remanence_session_free(session_raw);
}

/// Where the flux cycle lays its artifact, given by the caller so the
/// harness owns the scratch directory.
std::string artifact_path;

/// The flux ladder, which is the newest set of handles and the one whose
/// rungs each own private session storage: an image, the bitstream
/// materialized from it, the bytestream above that, and a rendition
/// report. Every one of them is released by scope exit alone.
void flux_cycle()
{
    remanence::FluxImage image = remanence::FluxImage::open(artifact_path);
    (void)image.holes();
    (void)image.orbits();
    (void)image.provenance();

    remanence::C1541Bitstream bits = image.materialize_c1541_bitstream();
    (void)bits.locations();
    (void)bits.evidence();
    (void)bits.declared_losses();

    remanence::C1541Bytestream bytes = bits.materialize_bytestream();
    (void)bytes.locations();
    (void)bytes.evidence();

    // Nothing frames a record here, so this refuses — and the refusal's
    // message and rule are freed on the way into the exception exactly
    // as any other's are.
    try {
        (void)bytes.recognize_sectors();
    } catch (const remanence::Error&) {
    }

    try {
        remanence::P64Report p64 = image.describe_p64();
        (void)p64.half_tracks();
        (void)p64.declared_losses();
        (void)p64.evidence();
    } catch (const remanence::Error&) {
    }
}

void measure(const char* what, void (*cycle)())
{
    cycle(); /* warm-up: settle whatever initialises lazily */

    const std::int64_t before = remanence_probe_live_allocations();
    for (int at = 0; at < CYCLES; at += 1) {
        cycle();
    }
    const std::int64_t after = remanence_probe_live_allocations();
    const std::int64_t leaked = after - before;

    if (leaked > 0) {
        failures += 1;
        std::printf("  FAIL %s leaked %lld blocks over %d cycles (%.2f per cycle): a "
                    "destructor must give back what its constructor took\n",
                    what, static_cast<long long>(leaked), CYCLES,
                    static_cast<double>(leaked) / CYCLES);
    } else {
        std::printf("  ok   %s: %lld live blocks either side of %d cycles\n", what,
                    static_cast<long long>(before), CYCLES);
    }
}

} // namespace

int main(int argc, char** argv)
{
    if (argc < 2) {
        std::printf("usage: wrapper_leaks <scratch-directory>\n");
        return 2;
    }

    measure("a session and its records, released by scope exit", handles_cycle);
    measure("a refusal's message and rule", refusal_cycle);
    measure("a handle released to a C caller", released_cycle);

    // The flux ladder is the one cycle that needs an artifact, so it
    // lays one: the format's own worked example, which is built here
    // rather than fetched and needs no fixture.
    artifact_path = std::string{argv[1]} + "/leak-cycle.remanence";
    if (!worked_example::place(artifact_path)) {
        std::printf("  FAIL cannot lay the artifact at %s\n", artifact_path.c_str());
        return 1;
    }
    measure("the flux ladder, released by scope exit", flux_cycle);
    std::remove(artifact_path.c_str());

    std::printf("  %d failures\n", failures);
    return failures == 0 ? 0 : 1;
}
