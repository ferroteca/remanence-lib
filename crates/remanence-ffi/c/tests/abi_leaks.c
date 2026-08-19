/*
 * SPDX-FileCopyrightText: 2026 Paul Galbraith
 * SPDX-License-Identifier: GPL-3.0-only
 */

/*
 * Every handle gives back what its creation took (S2, D47).
 *
 * This is the `_free` discipline asserted from C, which is the only
 * place it can be asserted honestly: the allocations are Rust's, made
 * inside the cdylib, so a C-side leak checker never sees them. The
 * library counts them instead and exports the count, and this program
 * reads it either side of a fixed number of create/destroy cycles.
 *
 * The symbol is declared here rather than included: it is test-only,
 * lives behind the library's `leak-probe` feature, and is deliberately
 * absent from the generated header because it is not part of S2.
 *
 * **The first cycle is a warm-up whose count is discarded.** A library
 * settles lazily-initialised state on first use — catalogs, tables — and
 * that is allocation which is never freed and never should be. What a
 * leak looks like is the count rising *per cycle* after that, so the
 * baseline is taken once the settling is done.
 */

#include <remanence.h>

#include <stdint.h>
#include <stdio.h>

/* Exported by the cdylib only under its `leak-probe` feature. */
extern int64_t remanence_probe_live_allocations(void);

/* Enough cycles that a one-block-per-cycle leak is unmistakable. */
#define CYCLES 8

static int failures = 0;

/* One full create-and-release round over a real artifact. */
static void discovery_cycle(const char *path)
{
    RemanenceErrorCategory category = REMANENCE_ERROR_CATEGORY_IO;
    char *message = NULL;
    char *rule = NULL;

    RemanenceDiscovery *discovery = remanence_discover_media(
        path, REMANENCE_ACCESS_INTENT_READ, &category, &message, &rule);

    if (discovery == NULL) {
        printf("  FAIL could not discover '%s': %s\n", path,
               message != NULL ? message : "(no message)");
        failures += 1;
        remanence_string_free(message);
        remanence_string_free(rule);
        return;
    }

    /* Reading owned strings must not allocate anything the caller owns. */
    (void)remanence_discovery_image_format(discovery);
    (void)remanence_discovery_article(discovery);
    (void)remanence_discovery_size(discovery);

    /* An assurance is its own owned handle with its own free. */
    RemanenceAssurance *assurance = remanence_discovery_assurance(discovery);
    if (assurance != NULL) {
        remanence_assurance_free(assurance);
    }

    remanence_discovery_free(discovery);
}

/* One refusal, whose message the caller owns and must free. */
static void refusal_cycle(void)
{
    RemanenceErrorCategory category = REMANENCE_ERROR_CATEGORY_IO;
    char *message = NULL;
    char *rule = NULL;

    RemanenceDiscovery *discovery = remanence_discover_media(
        "no-such-artifact-anywhere.img", REMANENCE_ACCESS_INTENT_READ,
        &category, &message, &rule);

    if (discovery != NULL) {
        remanence_discovery_free(discovery);
    }
    remanence_string_free(message);
    remanence_string_free(rule);
}

static void measure(const char *what, void (*cycle)(const char *),
                    const char *path)
{
    cycle(path); /* warm-up: settle whatever initialises lazily */

    int64_t before = remanence_probe_live_allocations();
    for (int at = 0; at < CYCLES; at += 1) {
        cycle(path);
    }
    int64_t after = remanence_probe_live_allocations();
    int64_t leaked = after - before;

    if (leaked > 0) {
        failures += 1;
        printf("  FAIL %s leaked %lld blocks over %d cycles (%.2f per "
               "cycle): every handle's free must give back what its "
               "creation took\n",
               what, (long long)leaked, CYCLES, (double)leaked / CYCLES);
    } else {
        printf("  ok   %s: %lld live blocks either side of %d cycles\n", what,
               (long long)before, CYCLES);
    }
}

static void refusal_adapter(const char *unused)
{
    (void)unused;
    refusal_cycle();
}

int main(int argc, char **argv)
{
    if (argc < 2) {
        printf("usage: abi_leaks <artifact>\n");
        return 2;
    }

    measure("a discovery opened and released", discovery_cycle, argv[1]);
    measure("a refusal's message freed", refusal_adapter, NULL);

    printf("  %d failures\n", failures);
    return failures == 0 ? 0 : 1;
}
