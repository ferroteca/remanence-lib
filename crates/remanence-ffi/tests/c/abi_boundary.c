/*
 * SPDX-FileCopyrightText: 2026 Paul Galbraith
 * SPDX-License-Identifier: GPL-3.0-only
 */

/*
 * The C ABI exercised as a C caller actually meets it (S2, D45).
 *
 * The FFI crate's own tests call the `extern "C"` functions from Rust,
 * which never crosses the boundary: no header, no C compiler, no C
 * calling convention, and no chance for a `#[repr(C)]` mistake to show.
 * This program is the other side of that boundary. It is compiled by a C
 * compiler against the generated header, linked against the built
 * library, and run.
 *
 * It takes one group name and runs that group, so each group is a named
 * test on the Rust side rather than one pass-or-fail lump. Failures
 * print what was expected and keep going, so one run reports everything
 * wrong with a group instead of only the first thing.
 */

#include <remanence.h>

#include <stdio.h>
#include <string.h>

static int failures = 0;
static int checks = 0;

#define CHECK(condition, ...)                                                  \
    do {                                                                       \
        checks += 1;                                                           \
        if (!(condition)) {                                                    \
            failures += 1;                                                     \
            printf("  FAIL %s:%d: ", __FILE__, __LINE__);                      \
            printf(__VA_ARGS__);                                               \
            printf("\n");                                                      \
        }                                                                      \
    } while (0)

/* ------------------------------------------------------------ catalogs
 *
 * Pure lookups: no artifact, no I/O, no ownership. They are the
 * cheapest thing that can only work if `size_t` and `const char *`
 * cross correctly, so they fail first and loudest when the ABI itself
 * is wrong.
 */
static void group_catalogs(void)
{
    size_t formats = remanence_format_count();
    CHECK(formats > 0, "the release claims no formats at all");

    for (size_t at = 0; at < formats; at += 1) {
        const char *id = remanence_format_id(at);
        const char *name = remanence_format_name(at);
        CHECK(id != NULL, "format %zu has no id", at);
        CHECK(name != NULL, "format %zu has no name", at);
        if (id != NULL) {
            CHECK(strlen(id) > 0, "format %zu has an empty id", at);
        }
    }

    /* Out of range answers null rather than reading past the claim. */
    CHECK(remanence_format_id(formats) == NULL,
          "an out-of-range format id did not answer null");
    CHECK(remanence_format_name(formats) == NULL,
          "an out-of-range format name did not answer null");

    size_t types = remanence_partition_type_count();
    CHECK(types > 0, "the release claims no partition types");
    CHECK(remanence_partition_type_id(types) == NULL,
          "an out-of-range partition type id did not answer null");

    size_t schemes = remanence_partition_scheme_count();
    CHECK(schemes > 0, "the release claims no partition schemes");

    size_t conditions = remanence_assurance_condition_count();
    CHECK(conditions > 0, "the release claims no assurance conditions");
    for (size_t at = 0; at < conditions; at += 1) {
        CHECK(remanence_assurance_condition_name(at) != NULL,
              "assurance condition %zu has no name", at);
    }
}

/* -------------------------------------------------------- the version
 *
 * One `const char *` the library owns and the caller must not free.
 */
static void group_version(void)
{
    const char *version = remanence_version();
    CHECK(version != NULL, "no version string");
    if (version != NULL) {
        CHECK(strlen(version) > 0, "the version string is empty");
    }
    CHECK(remanence_default_cache_bytes() > 0,
          "the default cache bound is zero");
}

/* --------------------------------------------------------- a refusal
 *
 * The out-parameter contract, which is the part of this ABI a C caller
 * is most likely to get wrong and the part nothing else tests: on
 * failure the category is written, a message is allocated for the
 * caller to free, and the rule is set or left null.
 */
static void group_refusal(void)
{
    RemanenceErrorCategory category = REMANENCE_ERROR_CATEGORY_IO;
    char *message = NULL;
    char *rule = NULL;

    RemanenceDiscovery *discovery = remanence_discover_media(
        "no-such-artifact-anywhere.img", REMANENCE_ACCESS_INTENT_READ,
        &category, &message, &rule);

    CHECK(discovery == NULL, "opening a missing artifact answered a discovery");
    CHECK(message != NULL, "a refusal set no message");
    if (message != NULL) {
        CHECK(strlen(message) > 0, "a refusal set an empty message");
    }

    /* Freeing an allocated message, and a null one, are both defined. */
    remanence_string_free(message);
    remanence_string_free(rule);
    remanence_string_free(NULL);

    if (discovery != NULL) {
        remanence_discovery_free(discovery);
    }
}

/* ------------------------------------------------- a real artifact
 *
 * The whole lifecycle a C caller performs: claim by name, read what was
 * established, release. `path` is the artifact's own; the strings are
 * the discovery's and must not be freed.
 */
static void group_discovery(const char *path)
{
    RemanenceErrorCategory category = REMANENCE_ERROR_CATEGORY_IO;
    char *message = NULL;
    char *rule = NULL;

    RemanenceDiscovery *discovery = remanence_discover_media(
        path, REMANENCE_ACCESS_INTENT_READ, &category, &message, &rule);

    if (discovery == NULL) {
        failures += 1;
        printf("  FAIL could not discover '%s': %s\n", path,
               message != NULL ? message : "(no message)");
        remanence_string_free(message);
        remanence_string_free(rule);
        return;
    }
    checks += 1;

    const char *format = remanence_discovery_image_format(discovery);
    const char *article = remanence_discovery_article(discovery);
    CHECK(format != NULL, "a discovery answered no image format");
    CHECK(article != NULL, "a discovery answered no article");
    CHECK(remanence_discovery_size(discovery) > 0,
          "a discovery answered a zero size");

    /* A read intent must come back read-only, never wider. */
    CHECK(remanence_discovery_mode(discovery) == REMANENCE_ACCESS_MODE_READ_ONLY,
          "a read-intent discovery did not answer read-only");

    /* The device families that would take it, by count and index. */
    size_t accepting = remanence_discovery_accepting_device_count(discovery);
    for (size_t at = 0; at < accepting; at += 1) {
        CHECK(remanence_discovery_accepting_device(discovery, at) != NULL,
              "accepting device %zu has no name", at);
    }
    CHECK(remanence_discovery_accepting_device(discovery, accepting) == NULL,
          "an out-of-range accepting device did not answer null");

    /* The assurance is an owned handle with its own free. */
    RemanenceAssurance *assurance = remanence_discovery_assurance(discovery);
    CHECK(assurance != NULL, "a discovery answered no assurance");
    if (assurance != NULL) {
        CHECK(remanence_assurance_outcome(assurance)
                  == REMANENCE_ASSURANCE_OUTCOME_VERIFIED,
              "a sound artifact did not verify");
        remanence_assurance_free(assurance);
    }

    remanence_discovery_free(discovery);
}

/* ------------------------------------------------------ null handling
 *
 * Accessors are documented to answer on a null handle rather than
 * dereferencing it. A C caller that gets this wrong crashes the host
 * process, so it is checked from C where a crash is a real crash.
 */
static void group_nulls(void)
{
    CHECK(remanence_discovery_path(NULL) == NULL,
          "a null discovery answered a path");
    CHECK(remanence_discovery_article(NULL) == NULL,
          "a null discovery answered an article");
    CHECK(remanence_discovery_size(NULL) == 0,
          "a null discovery answered a non-zero size");
    CHECK(remanence_discovery_accepting_device_count(NULL) == 0,
          "a null discovery answered a device count");

    /* Freeing null is defined for every owned handle. */
    remanence_discovery_free(NULL);
    remanence_assurance_free(NULL);
    remanence_session_free(NULL);
    remanence_string_free(NULL);
}

/* --------------------------------------------- the machine's device set
 *
 * A machine is configuration: devices in the order they were attached,
 * each answering to an identity the library assigns. What this checks is
 * that the set behaves from C as it does from Rust -- a slot is filled
 * once, an attachment resolves to the device in it, releasing frees the
 * slot, and every accessor answers on null rather than dereferencing it.
 */
static void group_machine(void)
{
    RemanenceErrorCategory category = REMANENCE_ERROR_CATEGORY_IO;
    char *message = NULL;
    char *rule = NULL;

    RemanenceSession *session = remanence_session_new();
    CHECK(session != NULL, "a session could not be created");

    RemanenceMachine *machine =
        remanence_session_add_machine(session, "pc", &category, &message, &rule);
    CHECK(machine != NULL, "a machine could not be added");

    const char *identity = remanence_machine_identity(machine);
    CHECK(identity != NULL && strcmp(identity, "pc") == 0,
          "the machine did not carry the identity it was given");

    /* A fresh machine holds nothing until a device is added to it. */
    CHECK(remanence_machine_device_count(machine) == 0,
          "a fresh machine already held a device");

    RemanenceDevice *first =
        remanence_machine_add_device(machine, "mbr-sector-hd", &category, &message, &rule);
    CHECK(first != NULL, "a hard drive could not be added");
    CHECK(remanence_machine_device_count(machine) == 1,
          "the added device was not counted");

    /* The attachment identity is the bay's prefix plus its index, and it
     * is the library's to assign rather than the caller's to choose. */
    char *attachment = NULL;
    CHECK(remanence_machine_device_attachment(machine, 0, &attachment),
          "the first device answered no attachment identity");
    if (attachment != NULL) {
        CHECK(strcmp(attachment, "hdd0") == 0,
              "the first hard drive was not attached at hdd0");
        CHECK(remanence_machine_device(machine, attachment) != NULL,
              "the attachment did not resolve to the device in it");
        remanence_string_free(attachment);
        attachment = NULL;
    }

    /* One bay holds one drive: the taken slot is refused by name. */
    CHECK(remanence_machine_add_device_at(machine, "gpt-hd", 0, &category, &message,
                                          &rule) == NULL,
          "a taken slot accepted a second drive");
    CHECK(message != NULL, "the refusal carried no message");
    if (message != NULL) {
        CHECK(strstr(message, "hdd0") != NULL,
              "the refusal did not name the slot asked for");
        remanence_string_free(message);
        message = NULL;
    }
    remanence_string_free(rule);
    rule = NULL;

    /* Releasing frees the slot; releasing an empty one is refused. */
    CHECK(remanence_machine_release_device(machine, "hdd0", &category, &message, &rule),
          "the device could not be released");
    CHECK(remanence_machine_device_count(machine) == 0,
          "the released device was still counted");
    CHECK(!remanence_machine_release_device(machine, "hdd0", &category, &message, &rule),
          "releasing an empty slot was accepted");
    remanence_string_free(message);
    message = NULL;
    remanence_string_free(rule);
    rule = NULL;

    /* Every accessor answers on null rather than dereferencing it. */
    CHECK(remanence_machine_identity(NULL) == NULL,
          "a null machine answered an identity");
    CHECK(remanence_machine_device_count(NULL) == 0,
          "a null machine answered a device count");
    CHECK(remanence_machine_device(NULL, "hdd0") == NULL,
          "a null machine resolved an attachment");
    CHECK(!remanence_machine_device_attachment(NULL, 0, &attachment),
          "a null machine answered an attachment identity");

    remanence_session_free(session);
}

int main(int argc, char **argv)
{
    if (argc < 2) {
        printf("usage: abi_boundary <group> [artifact]\n");
        return 2;
    }

    const char *group = argv[1];
    if (strcmp(group, "catalogs") == 0) {
        group_catalogs();
    } else if (strcmp(group, "version") == 0) {
        group_version();
    } else if (strcmp(group, "refusal") == 0) {
        group_refusal();
    } else if (strcmp(group, "nulls") == 0) {
        group_nulls();
    } else if (strcmp(group, "machine") == 0) {
        group_machine();
    } else if (strcmp(group, "discovery") == 0) {
        if (argc < 3) {
            printf("the discovery group needs an artifact path\n");
            return 2;
        }
        group_discovery(argv[2]);
    } else {
        printf("no such group: %s\n", group);
        return 2;
    }

    printf("  %d checks, %d failures\n", checks, failures);
    return failures == 0 ? 0 : 1;
}
