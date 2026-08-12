/*
 * SPDX-FileCopyrightText: 2026 Paul Galbraith
 * SPDX-License-Identifier: GPL-3.0-only
 */

/* Example C consumer of the remanence C ABI: mirrors the C++ CLI front-end.
 *
 * Build (MinGW, from the workspace root, after `cargo build -p remanence-ffi`):
 *   gcc crates/remanence-ffi/examples/identify.c target/debug/remanence_ffi.dll \
 *       -I crates/remanence-ffi/include -o identify.exe
 */

#include <ctype.h>
#include <inttypes.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#ifdef _WIN32
#include <windows.h>
#else
#include <fcntl.h>
#include <unistd.h>
#endif

#include "remanence.h"

static const char *layer_kind_name(RemanenceLayerKind kind) {
    switch (kind) {
        case REMANENCE_LAYER_KIND_ARCHIVE: return "archive";
        case REMANENCE_LAYER_KIND_IMAGE: return "image";
        case REMANENCE_LAYER_KIND_PHYSICAL_MEDIA: return "physical-media";
        case REMANENCE_LAYER_KIND_FILESYSTEM: return "filesystem";
        case REMANENCE_LAYER_KIND_UNKNOWN: return "unknown";
    }
    return "unknown";
}

/* Reports a refusal the way the ABI states it: the stable category, which
 * says how to behave; the rule identity where the refusal came from an
 * enumerated rule set, absent otherwise; and the human diagnostic. Both
 * strings are ours to free, and this consumes them. */
static void report_error(const char *what, RemanenceErrorCategory category,
                         char *message, char *rule) {
    fprintf(stderr, "%s (category %d", what, (int)category);
    if (rule != NULL) {
        fprintf(stderr, ", rule %s", rule);
    }
    fprintf(stderr, "): %s\n", message != NULL ? message : "unknown");
    remanence_string_free(message);
    remanence_string_free(rule);
}

static void print_size(const RemanenceIdentification *identification, size_t index) {
    uint64_t current = 0;
    uint64_t expected = 0;
    printf("      size: ");
    if (remanence_layer_current_bytes(identification, index, &current)) {
        printf("%" PRIu64 " bytes", current);
    } else {
        printf("unknown");
    }
    if (remanence_layer_expected_bytes(identification, index, &expected)) {
        printf(" (expected %" PRIu64 ")", expected);
    }
    printf("\n");
}

/* An ASCII case-insensitive compare, spelled here so the example needs no
 * platform string extension. */
static int _stricmp_portable(const char *a, const char *b) {
    while (*a != '\0' && *b != '\0') {
        int left = tolower((unsigned char)*a++);
        int right = tolower((unsigned char)*b++);
        if (left != right) {
            return left - right;
        }
    }
    return (unsigned char)*a - (unsigned char)*b;
}

/* This caller's own open, handed to the library with the format it is
 * declared to be. The library takes ownership of the handle: it is
 * closed when the medium is released or the session is freed, never by
 * us. Answers 0 where the artifact cannot be opened at all. */
static intptr_t open_source(const char *path) {
#ifdef _WIN32
    HANDLE handle = CreateFileA(path, GENERIC_READ, FILE_SHARE_READ, NULL, OPEN_EXISTING,
                                FILE_ATTRIBUTE_NORMAL, NULL);
    return handle == INVALID_HANDLE_VALUE ? 0 : (intptr_t)handle;
#else
    int fd = open(path, O_RDONLY);
    return fd < 0 ? 0 : (intptr_t)fd;
#endif
}

/* A name recovered from a handle serves location alone, and a handle
 * this host cannot name has none -- which is an answer rather than a
 * failure, so it is printed as one. */
static const char *name_or(const char *name) {
    return name == NULL ? "(this handle has no recoverable name)" : name;
}

/* The declaration this example makes for an artifact, from its
 * extension. **A real consumer declares what it knows it has**; reading
 * the extension is this example's stand-in for that knowledge, and it is
 * still a declaration the library checks rather than a probe. */
static const char *declared_format(const char *path) {
    const char *dot = strrchr(path, '.');
    if (dot != NULL) {
        for (size_t i = 0; i < remanence_format_count(); ++i) {
            const char *id = remanence_format_id(i);
            if (id != NULL && _stricmp_portable(dot + 1, id) == 0) {
                return id;
            }
        }
    }
    /* Bytes, and nothing else: the reading every artifact bears. */
    return "raw";
}

/* An archive's grammar, declared the same way. */
static const char *archive_format(const char *path) {
    const char *format = declared_format(path);
    return strcmp(format, "raw") == 0 ? "zip" : format;
}

/* The device this example declares its artifact was recorded by.
 *
 * A format that records one device type carries it bare, and passing
 * null is how a C caller says so. One that records several needs the
 * caller to choose, and this example chooses the block-addressed hard
 * drive for them -- **a real consumer states the drive the disk came
 * out of**, which is a machine fact it knows and this example does not.
 */
static const char *declared_device(const char *format) {
    for (size_t i = 0; i < remanence_format_count(); ++i) {
        const char *id = remanence_format_id(i);
        if (id == NULL || strcmp(id, format) != 0) {
            continue;
        }
        return remanence_format_device_count(i) == 1 ? NULL
                                                     : remanence_format_device(i, 1);
    }
    return NULL;
}

/* The block size a declaration carries, which only the raw reading
 * takes: bytes have no block meaning without one, and every other
 * format records its own. */
static uint64_t declared_block_bytes(const char *format) {
    for (size_t i = 0; i < remanence_format_count(); ++i) {
        const char *id = remanence_format_id(i);
        if (id != NULL && strcmp(id, format) == 0) {
            return remanence_format_takes_block_bytes(i) ? 512 : 0;
        }
    }
    return 0;
}

/* The image container format the adapter recognized, and the version the
 * formats that declare one carry. A version accessor answers 0 for an
 * image of any other format, so each is read only under its own format. */
static void print_medium_format(const RemanenceMedium *medium) {
    RemanenceDiskFormat format;
    if (!remanence_medium_format(medium, &format)) {
        /* A medium that is no disk image -- an archive -- presents no
         * disk to state the format or the size of. */
        return;
    }
    switch (format) {
        case REMANENCE_DISK_FORMAT_QCOW2:
            printf("Format:  qcow2 (version %" PRIu32 ")\n",
                   remanence_medium_qcow2_version(medium));
            break;
        case REMANENCE_DISK_FORMAT_VDI:
            printf("Format:  vdi (version %" PRIu32 ".%" PRIu32 ")\n",
                   remanence_medium_vdi_version_major(medium),
                   remanence_medium_vdi_version_minor(medium));
            break;
        case REMANENCE_DISK_FORMAT_RAW:
            printf("Format:  raw\n");
            break;
    }
    printf("Size:    %" PRIu64 " bytes\n", remanence_medium_size(medium));
}

/* What the open established about the evidence beneath it (P28), and what
 * that narrows. A verified medium says so in one line; a degraded one
 * states the condition, the evidence, the extents that read, and the
 * access it actually has -- before anything is read from it. */
static void print_assurance(const RemanenceMedium *medium) {
    RemanenceAssurance *assurance = remanence_medium_assurance(medium);
    if (assurance == NULL) {
        return;
    }

    RemanenceAssuranceOutcome outcome = remanence_assurance_outcome(assurance);
    const char *condition = remanence_assurance_condition(assurance);
    printf("Assurance: %s", outcome == REMANENCE_ASSURANCE_OUTCOME_VERIFIED ? "verified"
                                                                            : "degraded");
    if (condition != NULL) {
        printf(" (%s)", condition);
    }
    printf(", %s\n",
           remanence_assurance_access_mode(assurance) == REMANENCE_ACCESS_MODE_READ_WRITE
               ? "read-write"
               : "read-only");

    if (outcome != REMANENCE_ASSURANCE_OUTCOME_VERIFIED) {
        uint64_t declared = 0;
        uint64_t observed = 0;
        if (remanence_assurance_declared_bytes(assurance, &declared) &&
            remanence_assurance_observed_bytes(assurance, &observed)) {
            printf("  declared %" PRIu64 " bytes, source holds %" PRIu64 "\n", declared,
                   observed);
        }
        for (size_t i = 0; i < remanence_assurance_readable_count(assurance); ++i) {
            uint64_t start = 0;
            uint64_t end = 0;
            if (remanence_assurance_readable(assurance, i, &start, &end)) {
                printf("  readable: %" PRIu64 "..%" PRIu64 "\n", start, end);
            }
        }
        for (size_t i = 0; i < remanence_assurance_evidence_count(assurance); ++i) {
            printf("  * %s\n", remanence_assurance_evidence(assurance, i));
        }
    }

    remanence_assurance_free(assurance);
}

/* The medium's own partition pool -- what its content is reached through.
 *
 * The scheme is evidence: a medium that recorded no table says so, and
 * the *direct partition* stands in its place. That one member is the
 * library's own composition of the whole content, which is why it accounts
 * for itself in provenance and carries no evidence at all -- a composition
 * act is stated as one, never offered as something the medium said.
 *
 * The ordinals are the scheme's own and the indices are this walk's: a
 * partition carrying an issue keeps its number, so the ones behind it
 * never renumber, and it is the ordinal that names a partition. */
static void print_partitions(RemanenceMedium *medium) {
    const char *scheme = remanence_medium_partition_scheme(medium);
    size_t count = remanence_medium_partition_count(medium);
    printf("\nPartitions (%zu, %s):\n", count,
           scheme != NULL ? scheme : "this medium records no scheme");
    for (size_t i = 0; i < count; ++i) {
        uint32_t ordinal = 0;
        uint8_t type_byte = 0;
        uint64_t start = 0;
        uint64_t length = 0;
        if (!remanence_medium_partition_ordinal(medium, i, &ordinal)) {
            continue;
        }
        RemanencePartition *partition = remanence_medium_partition(medium, ordinal);
        if (partition == NULL) {
            /* Absence is an answer and the outs are untouched for it, so
             * there is nothing here to report as a failure. */
            continue;
        }
        printf("  %" PRIu32 ": %s%s%s", remanence_partition_ordinal(partition),
               remanence_partition_placement(partition),
               remanence_partition_is_direct(partition) ? " [the direct partition]" : "",
               remanence_partition_active(partition) ? ", active" : "");
        /* The raw value beside what it *declares*. The reading describes
         * the declaration and never the content, and it is present even
         * for a type this release will not read -- the partition a caller
         * most needs explained is the one the library declines. */
        if (remanence_partition_type_byte(partition, &type_byte)) {
            const char *reading = remanence_partition_type_reading(partition);
            printf(", type 0x%02x (%s)%s", (unsigned int)type_byte,
                   reading != NULL ? reading : "no reading",
                   remanence_partition_is_claimed(partition) ? "" : " [not read]");
        }
        if (remanence_partition_start_bytes(partition, &start) &&
            remanence_partition_length_bytes(partition, &length)) {
            printf(", %" PRIu64 " bytes at %" PRIu64, length, start);
        }
        printf(", %s%s\n",
               remanence_partition_is_addressable(partition) ? "addressable" : "no extent",
               remanence_partition_bears_namespace(partition) ? ", bears a namespace" : "");

        const char *provenance = remanence_partition_provenance(partition);
        if (provenance != NULL) {
            printf("     composed: %s\n", provenance);
        }
        for (size_t line = 0; line < remanence_partition_evidence_count(partition); ++line) {
            printf("     * %s\n", remanence_partition_evidence(partition, line));
        }
        /* A partition carrying a refusal is still a partition and still
         * keeps its place. */
        const char *issue = remanence_partition_issue(partition);
        if (issue != NULL) {
            printf("     issue: %s\n", issue);
        }
        remanence_partition_free(partition);
    }
}

/* The namespace this example reads, found by walking that same pool.
 *
 * A partition whose declared type *determines* a namespace opens the
 * plain door and this caller declares nothing. Nothing determines one
 * over a direct partition, so there the reading is declared -- and a
 * declaration is checked rather than probed, which is why the refusal of
 * "fat" over an HDOS disk is an answer to that question and the next
 * declaration is a fresh one. This example takes whichever of the formats
 * the artifact turns out to bear.
 *
 * Answers NULL where nothing did, handing back the last refusal for the
 * caller to report and to free. */
static RemanenceSpace *open_namespace(RemanenceMedium *medium,
                                      RemanenceErrorCategory *last_category, char **last_error,
                                      char **last_rule) {
    size_t count = remanence_medium_partition_count(medium);
    *last_error = NULL;
    *last_rule = NULL;
    for (size_t i = 0; i < count; ++i) {
        RemanenceErrorCategory category = REMANENCE_ERROR_CATEGORY_UNSUPPORTED;
        char *error = NULL;
        char *rule = NULL;
        uint32_t ordinal = 0;
        RemanenceSpace *space = NULL;

        if (!remanence_medium_partition_ordinal(medium, i, &ordinal)) {
            continue;
        }
        RemanencePartition *partition = remanence_medium_partition(medium, ordinal);
        if (partition == NULL) {
            continue;
        }
        if (remanence_partition_bears_namespace(partition)) {
            space = remanence_partition_filesystem(partition, &category, &error, &rule);
        } else if (remanence_partition_is_direct(partition)) {
            space = remanence_partition_filesystem_as(partition, "fat", &category, &error, &rule);
            if (space == NULL) {
                remanence_string_free(error);
                remanence_string_free(rule);
                error = NULL;
                rule = NULL;
                space = remanence_partition_filesystem_as(partition, "hdos", &category, &error,
                                                          &rule);
            }
        }
        /* The plain door hands back the node whether or not the reading
         * the type determined held: a FAT partition the content does not
         * bear out composes a space with the namespace absent on it. That
         * is not the node this walk is after, so it keeps looking. */
        if (space != NULL && !remanence_filesystem_has_namespace(space)) {
            remanence_space_free(space);
            space = NULL;
        }
        /* The space names its provider and never this handle, so the
         * partition is done with either way. */
        remanence_partition_free(partition);
        if (space != NULL) {
            remanence_string_free(error);
            remanence_string_free(rule);
            return space;
        }
        /* Keep the last refusal to show, and never two at once. A
         * partition with neither vantage refused nothing -- it composed
         * nothing to refuse -- so it leaves the kept one alone. */
        if (error != NULL) {
            remanence_string_free(*last_error);
            remanence_string_free(*last_rule);
            *last_category = category;
            *last_error = error;
            *last_rule = rule;
        }
    }
    return NULL;
}

static const char *outcome_name(RemanenceLetterOutcome outcome) {
    switch (outcome) {
        case REMANENCE_LETTER_OUTCOME_VOLUME: return "volume";
        case REMANENCE_LETTER_OUTCOME_DECLARED_DEVICE: return "declared-device";
        case REMANENCE_LETTER_OUTCOME_PHANTOM: return "phantom";
        case REMANENCE_LETTER_OUTCOME_UNDETERMINED: return "undetermined";
    }
    return "undetermined";
}

/* Composes the drive letters a DOS machine holding this one medium would
 * have presented. The machine facts are ours to assert — this medium is the
 * first fixed device's — and the assignment rule is the library's.
 * No variant is stated here, so a letter the claimed rules disagree on
 * comes back undetermined rather than guessed. */
static void show_drive_letters(RemanenceMedium *medium) {
    RemanenceErrorCategory error_category;
    char *error = NULL;
    char *error_rule = NULL;

    RemanenceDiskReport *report =
        remanence_medium_inspect(medium, &error_category, &error, &error_rule);
    if (report == NULL) {
        report_error("\nerror inspecting device", error_category, error, error_rule);
        return;
    }

    RemanenceDosMachine *machine = remanence_dos_machine_new();
    if (!remanence_dos_machine_assert_fixed_disk(machine, 0, report, &error_category,
                                                 &error, &error_rule)) {
        report_error("\nerror asserting the machine", error_category, error, error_rule);
        remanence_dos_machine_free(machine);
        remanence_report_free(report);
        return;
    }

    RemanenceDriveMap *map =
        remanence_dos_machine_compose(machine, NULL, &error_category, &error, &error_rule);
    if (map == NULL) {
        report_error("\nerror composing drive letters", error_category, error, error_rule);
        remanence_dos_machine_free(machine);
        remanence_report_free(report);
        return;
    }

    size_t letters = remanence_drive_map_count(map);
    printf("\nDOS drive letters (%zu, %zu established) under:\n", letters,
           remanence_drive_map_established_count(map));
    for (size_t i = 0; i < remanence_drive_map_applied_rule_count(map); ++i) {
        printf("  rule %s\n", remanence_drive_map_applied_rule(map, i));
    }
    for (size_t i = 0; i < letters; ++i) {
        uint64_t volume = 0;
        uint32_t device_index = 0;
        RemanenceLetterOutcome outcome = remanence_drive_map_outcome(map, i);
        printf("  %c: %s", remanence_drive_map_letter(map, i), outcome_name(outcome));
        if (remanence_drive_map_device_index(map, i, &device_index)) {
            printf(" (%s %" PRIu32 ")", remanence_drive_map_device_kind(map, i), device_index);
        }
        if (remanence_drive_map_volume(map, i, &volume)) {
            printf(" volume %" PRIu64, volume);
        }
        if (outcome == REMANENCE_LETTER_OUTCOME_PHANTOM) {
            printf(" of %c:", remanence_drive_map_phantom_of(map, i));
        }
        const char *reason = remanence_drive_map_reason(map, i);
        if (reason != NULL) {
            printf(" -- %s", reason);
        }
        printf("\n");
    }

    printf("Provenance (not evidence):\n");
    for (size_t i = 0; i < remanence_drive_map_provenance_count(map); ++i) {
        printf("  * %s\n", remanence_drive_map_provenance(map, i));
    }

    remanence_drive_map_free(map);
    remanence_dos_machine_free(machine);
    remanence_report_free(report);
}

/* Lists what an archive holds, without reading any entry's data.
 *
 * An archive is a medium like any other: it loads into a device of its
 * own family, and its content is reached through the direct partition it
 * bears -- so this is the same walk a disk's filesystem takes, with no
 * archive journey of its own. */
static int list_archive(const char *path) {
    RemanenceErrorCategory error_category;
    char *error = NULL;
    char *error_rule = NULL;

    RemanenceSession *session = remanence_session_new();
    /* Whoever opens owns the lock: the source is this caller's own open
     * file, handed over with the format it is declared to be, and the
     * library takes ownership of the handle from here. */
    intptr_t source = open_source(path);
    if (source == 0) {
        fprintf(stderr, "error: cannot open '%s'\n", path);
        remanence_session_free(session);
        return EXIT_FAILURE;
    }
    const char *format = archive_format(path);
    RemanenceMedium *medium = remanence_session_load_media(
        session, source, format, declared_device(format), declared_block_bytes(format),
        &error_category, &error, &error_rule);
    if (medium == NULL) {
        report_error("error", error_category, error, error_rule);
        remanence_session_free(session);
        return EXIT_FAILURE;
    }

    /* An archive is a medium like any other, and may be seated in the
     * archive receiver -- which is configuration, not a step the content
     * verbs wait on. The receiver is no device type: an archive was
     * recorded by no device, so there is no recording for a type to
     * name. */
    RemanenceDevice *device = remanence_session_add_device(
        session, "archive", &error_category, &error, &error_rule);
    if (device == NULL || !remanence_device_insert(device, remanence_medium_id(medium),
                                                   &error_category, &error, &error_rule)) {
        report_error("error seating the archive", error_category, error, error_rule);
        remanence_session_free(session);
        return EXIT_FAILURE;
    }

    printf("Archive: %s\n", name_or(remanence_medium_path(medium)));
    printf("Device:  %s (%s)\n", remanence_device_attachment(device),
           remanence_device_slot(device));
    printf("Size:    %" PRIu64 " bytes\n\n", remanence_medium_image_size_bytes(medium));

    /* An archive records no partition scheme, so its pool holds exactly
     * one member: the direct partition at ordinal 0, which the library
     * composed rather than read. Its content *is* a namespace -- the
     * grammar that recognized the artifact already read the index -- so
     * the plain door opens and nothing is declared here. */
    RemanencePartition *partition = remanence_medium_partition(medium, 0);
    if (partition == NULL) {
        fprintf(stderr, "error: this medium's pool bears no direct partition\n");
        remanence_session_free(session);
        return EXIT_FAILURE;
    }
    RemanenceSpace *namespace =
        remanence_partition_filesystem(partition, &error_category, &error, &error_rule);
    /* The partition is a snapshot and the space re-resolves without it,
     * so it is done with here whether or not the door opened. */
    remanence_partition_free(partition);
    if (namespace == NULL) {
        /* Null with the outs untouched would be the namespace vantage
         * simply being absent; an archive always bears one, so anything
         * here is a refusal to report. */
        report_error("error reaching the namespace", error_category, error, error_rule);
        remanence_session_free(session);
        return EXIT_FAILURE;
    }
    if (!remanence_filesystem_has_namespace(namespace)) {
        fprintf(stderr, "error: this archive's content bears no namespace\n");
        remanence_space_free(namespace);
        remanence_session_free(session);
        return EXIT_FAILURE;
    }
    printf("Format:  %s\n", remanence_filesystem_kind(namespace));

    RemanenceEntryList *entries = remanence_filesystem_entries(
        namespace, "", &error_category, &error, &error_rule);
    if (entries == NULL) {
        report_error("error listing the archive", error_category, error, error_rule);
        remanence_space_free(namespace);
        remanence_session_free(session);
        return EXIT_FAILURE;
    }

    size_t entry_count = remanence_entry_count(entries);
    printf("Entries (%zu):\n", entry_count);
    for (size_t i = 0; i < entry_count; ++i) {
        printf("  %s%s\t%" PRIu64 " bytes\n",
               remanence_entry_name(entries, i),
               remanence_entry_kind(entries, i) == REMANENCE_ENTRY_KIND_DIRECTORY ? "/" : "",
               remanence_entry_size_bytes(entries, i));
    }

    remanence_entry_list_free(entries);
    remanence_space_free(namespace);
    remanence_session_free(session);
    return EXIT_SUCCESS;
}

/* Lists the devices this release claims, so a caller can see what a
 * machine's slots may be: one per device type in the catalog, plus the
 * archive receiver, which is no recording device at all. */
static void list_devices(void) {
    size_t count = remanence_device_slot_count();
    printf("Devices (%zu):\n", count);
    for (size_t i = 0; i < count; ++i) {
        const char *class_of = remanence_device_slot_class(i);
        const char *scheme = remanence_device_slot_scheme(i);
        printf("  %-16s %-38s slot %s%s%s%s%s\n",
               remanence_device_slot_id(i),
               remanence_device_slot_name(i),
               remanence_device_slot_prefix(i),
               class_of == NULL ? "" : ", class ",
               class_of == NULL ? "" : class_of,
               scheme == NULL ? "" : ", scheme ",
               scheme == NULL ? "" : scheme);
    }
}

/* Asks what one artifact is, before any machine has been configured for
 * it: the exact medium, the drives that would take it, and the drive the
 * image format declares for the disks it records. The discovery holds the
 * claim under which all that was established, so freeing it is what ends
 * that claim -- here, because this mode loads nothing afterwards. */
static int show_discovery(const char *path) {
    RemanenceErrorCategory error_category;
    char *error = NULL;
    char *error_rule = NULL;
    RemanenceDiscovery *discovery = remanence_discover_media(
        path, REMANENCE_ACCESS_INTENT_READ, &error_category, &error, &error_rule);
    if (discovery == NULL) {
        report_error("error", error_category, error, error_rule);
        return EXIT_FAILURE;
    }

    printf("Source:  %s\n", remanence_discovery_path(discovery));
    printf("Image:   %s\n", remanence_discovery_image_path(discovery));
    printf("Format:  %s (%s)\n", remanence_discovery_image_format_name(discovery),
           remanence_discovery_image_format(discovery));
    printf("Article: %s (%s)\n", remanence_discovery_article_name(discovery),
           remanence_discovery_article(discovery));
    printf("Size:    %" PRIu64 " bytes\n", remanence_discovery_size(discovery));

    size_t accepting = remanence_discovery_accepting_device_count(discovery);
    printf("Devices served it (%zu):\n", accepting);
    for (size_t i = 0; i < accepting; ++i) {
        printf("  %s\n", remanence_discovery_accepting_device(discovery, i));
    }
    /* Two different questions: where the medium could go, above, and
     * what wrote it, here. A format that records several device types
     * says which they are and asserts none -- nothing in the artifact
     * says which hard drive wrote a raw image, and a load declares it. */
    const char *recorded = remanence_discovery_device_type(discovery);
    printf("Recorded by: %s\n", recorded != NULL ? recorded : "nothing states which");
    size_t candidates = remanence_discovery_recorded_device_count(discovery);
    if (recorded == NULL && candidates > 0) {
        printf("Declare one of (%zu):\n", candidates);
        for (size_t i = 0; i < candidates; ++i) {
            printf("  %s\n", remanence_discovery_recorded_device(discovery, i));
        }
    }

    remanence_discovery_free(discovery);
    return EXIT_SUCCESS;
}

/* The remanence image: the flux family's physical stratum, read from its
 * own artifact rather than through a device. Block and flux are disjoint
 * families, so a flux artifact is reached through its own type exactly as
 * a capture set is -- there is no device to load it into. What crosses
 * the boundary is the image's shape; the points beneath it stay in the
 * library, streamed from private session storage under the declared
 * bound. */
/* A count is not an account: each entry says what was left behind and how
 * much of it there was. */
static void print_d64_loss(const RemanenceD64Report *report) {
    size_t losses = remanence_d64_report_declared_loss_count(report);
    printf("  not carried (%zu):%s\n", losses, losses == 0 ? " nothing" : "");
    for (size_t i = 0; i < losses; ++i) {
        printf("    %s: %s (%" PRIu64 ")\n",
               remanence_d64_report_declared_loss_code(report, i),
               remanence_d64_report_declared_loss_detail(report, i),
               remanence_d64_report_declared_loss_amount(report, i));
    }
}

static void print_g64_loss(const RemanenceG64Report *report) {
    size_t losses = remanence_g64_report_declared_loss_count(report);
    printf("  not carried (%zu):%s\n", losses, losses == 0 ? " nothing" : "");
    for (size_t i = 0; i < losses; ++i) {
        printf("    %s: %s (%" PRIu64 ")\n",
               remanence_g64_report_declared_loss_code(report, i),
               remanence_g64_report_declared_loss_detail(report, i),
               remanence_g64_report_declared_loss_amount(report, i));
    }
}

static int show_remanence_image(const char *path, const char *destination) {
    RemanenceErrorCategory error_category;
    char *error = NULL;
    char *error_rule = NULL;
    RemanenceImage *image =
        remanence_image_open(path, &error_category, &error, &error_rule);
    if (image == NULL) {
        report_error("error", error_category, error, error_rule);
        return EXIT_FAILURE;
    }

    printf("Source:  %s\n", remanence_image_path(image));
    printf("Format:  %s (%s)\n", remanence_image_format_name(image),
           remanence_image_format_id(image));
    printf("Medium:  %s\n", remanence_image_form_factor(image));
    printf("Angles:  %" PRIu64 " divisions per turn\n",
           remanence_image_angular_divisions(image));

    size_t holes = remanence_image_hole_count(image);
    printf("Holes:   %zu\n", holes);
    for (size_t i = 0; i < holes; ++i) {
        RemanenceImageHole hole;
        if (remanence_image_hole(image, i, &hole)) {
            printf("  centre %" PRIu64 "/%" PRIu64 ", extent %" PRIu64 "/%" PRIu64 "\n",
                   hole.center_numerator, hole.center_denominator,
                   hole.extent_numerator, hole.extent_denominator);
        }
    }

    size_t surfaces = remanence_image_surface_count(image);
    printf("Surfaces (%zu):", surfaces);
    for (size_t i = 0; i < surfaces; ++i) {
        uint64_t surface = 0;
        if (remanence_image_surface(image, i, &surface)) {
            printf(" %" PRIu64, surface);
        }
    }
    printf("\n");

    /* One recorded band at one radius. "Orbit", not "track": both the
     * flux community and the recording formats use "track" and mean
     * different radii by it. */
    size_t orbits = remanence_image_orbit_count(image);
    uint64_t points = 0;
    uint64_t unread = 0;
    printf("Orbits (%zu):\n", orbits);
    for (size_t i = 0; i < orbits; ++i) {
        RemanenceImageOrbit orbit;
        if (!remanence_image_orbit(image, i, &orbit)) {
            continue;
        }
        points += orbit.points;
        unread += orbit.unaligned_spans;
        /* The ends of the disk say most about it; the middle is more of
         * the same, so this prints the first three and the last. */
        if (i < 3 || i + 1 == orbits) {
            printf("  surface %" PRIu64 " radius %" PRIu64 " um: %" PRIu64
                   " points (%" PRIu64 " coherent, %" PRIu64 " unread)\n",
                   orbit.surface, orbit.radius_microns, orbit.points,
                   orbit.coherent_points, orbit.unaligned_spans);
        } else if (i == 3) {
            printf("  ...\n");
        }
    }
    printf("Total:   %" PRIu64 " points, %" PRIu64 " spans unread\n", points, unread);

    size_t provenance = remanence_image_provenance_count(image);
    printf("Known by (%zu):\n", provenance);
    for (size_t i = 0; i < provenance; ++i) {
        printf("  %s\n", remanence_image_provenance(image, i));
    }
    printf("Backing: %" PRIu64 " bytes, %" PRIu64 " resident\n",
           remanence_image_backing_bytes(image), remanence_image_resident_bytes(image));

    /* Writing is the other direction of the same claim. An existing
     * destination refuses by name rather than being overwritten, and the
     * P29 account comes back empty because this is the model's own
     * artifact: nothing of the image was left behind. */
    int status = EXIT_SUCCESS;
    if (destination != NULL) {
        RemanenceImageWriteReport *written = remanence_image_write(
            image, destination, &error_category, &error, &error_rule);
        if (written == NULL) {
            report_error("error", error_category, error, error_rule);
            status = EXIT_FAILURE;
        } else {
            printf("Wrote:   %s (%" PRIu64 " bytes, %" PRIu64 " orbits, %" PRIu64
                   " points)\n",
                   remanence_image_write_path(written),
                   remanence_image_write_artifact_bytes(written),
                   remanence_image_write_orbits(written),
                   remanence_image_write_points(written));
            size_t losses = remanence_image_write_declared_loss_count(written);
            printf("Not carried (%zu):%s\n", losses, losses == 0 ? " nothing" : "");
            for (size_t i = 0; i < losses; ++i) {
                printf("  %s: %s (%" PRIu64 ")\n",
                       remanence_image_write_declared_loss_code(written, i),
                       remanence_image_write_declared_loss_detail(written, i),
                       remanence_image_write_declared_loss_amount(written, i));
            }
            remanence_image_write_report_free(written);
        }
    }

    /* The C64 renditions, computed and not written. P29 acts where only
     * the destination varies, so each states what it will not carry
     * before anything exists to carry it. */
    RemanenceD64Report *d64 =
        remanence_image_describe_d64(image, &error_category, &error, &error_rule);
    if (d64 == NULL) {
        report_error("no d64", error_category, error, error_rule);
    } else {
        printf("d64:     %" PRIu32 " of %" PRIu32 " blocks, %" PRIu32
               " failed checksums, %" PRIu64 " bytes\n",
               remanence_d64_report_blocks_read(d64),
               remanence_d64_report_blocks_defined(d64),
               remanence_d64_report_failed_checksums(d64),
               remanence_d64_report_artifact_bytes(d64));
        print_d64_loss(d64);
        remanence_d64_report_free(d64);
    }

    RemanenceG64Report *g64 =
        remanence_image_describe_g64(image, &error_category, &error, &error_rule);
    if (g64 == NULL) {
        report_error("no g64", error_category, error, error_rule);
    } else {
        size_t slots = remanence_g64_report_half_track_count(g64);
        printf("g64:     %zu half-tracks, %" PRIu64 " bytes\n", slots,
               remanence_g64_report_artifact_bytes(g64));
        for (size_t i = 0; i < slots; ++i) {
            RemanenceG64HalfTrack slot;
            if (remanence_g64_report_half_track(g64, i, &slot) && (i < 2 || i + 1 == slots)) {
                printf("  slot %" PRIu64 ": %" PRIu64 " bits at zone %u%s\n", slot.index,
                       slot.bits, slot.speed_zone,
                       slot.clocked_at_nominal ? " (clocked at nominal)" : "");
            } else if (i == 2) {
                printf("  ...\n");
            }
        }
        print_g64_loss(g64);
        remanence_g64_report_free(g64);
    }

    RemanenceP64Report *p64 =
        remanence_image_describe_p64(image, &error_category, &error, &error_rule);
    if (p64 == NULL) {
        report_error("no p64", error_category, error, error_rule);
    } else {
        uint64_t pulses = 0;
        size_t slots = remanence_p64_half_track_count(NULL, p64);
        for (size_t i = 0; i < slots; ++i) {
            RemanenceP64HalfTrack slot;
            if (remanence_p64_half_track(NULL, p64, i, &slot)) {
                pulses += slot.pulses;
            }
        }
        printf("p64:     %zu half-tracks, %" PRIu64 " pulses\n", slots, pulses);
        remanence_p64_report_free(p64);
    }

    remanence_image_free(image);
    return status;
}

/* Reduces a KryoFlux capture set to a remanence image on the strength of
 * all the evidence rather than the choice of one revolution. The plan
 * computes the whole reduction and writes nothing, so the account is
 * complete before the image exists; executing it answers with the
 * family's ordinary image handle rather than a root of its own, and the
 * plan handle is consumed whether it succeeds or fails. */
static int reconstruct_capture(const char *path, unsigned long side) {
    RemanenceErrorCategory error_category;
    char *error = NULL;
    char *error_rule = NULL;
    RemanenceCaptureSet *set =
        remanence_capture_set_open(path, &error_category, &error, &error_rule);
    if (set == NULL) {
        report_error("error", error_category, error, error_rule);
        return EXIT_FAILURE;
    }

    RemanenceReconstructionPolicy policy;
    policy.side = (uint64_t)side;
    policy.recordings = REMANENCE_RECORDING_SELECTION_MEASURED;
    policy.declared_positions = NULL;
    policy.declared_position_count = 0;

    RemanenceReconstructionPlan *plan = remanence_capture_set_plan_reconstruction(
        set, &policy, &error_category, &error, &error_rule);
    if (plan == NULL) {
        report_error("error", error_category, error, error_rule);
        remanence_capture_set_free(set);
        return EXIT_FAILURE;
    }

    printf("Capture: %s\n", remanence_capture_set_path(set));
    printf("Side:    %" PRIu64 "\n", remanence_reconstruction_side(plan));
    printf("Swept:   %" PRIu32 " step positions\n",
           remanence_reconstruction_swept_positions(plan));

    size_t recorded = remanence_reconstruction_recorded_position_count(plan);
    printf("Records (%zu):", recorded);
    for (size_t i = 0; i < recorded; ++i) {
        uint64_t position = 0;
        if (remanence_reconstruction_recorded_position(plan, i, &position)) {
            printf(" %" PRIu64, position);
        }
    }
    printf("\n");

    /* One recorded band at one radius, measured rather than asserted:
     * the fat-track merge decides which of them enter the image. */
    size_t orbits = remanence_reconstruction_orbit_count(plan);
    size_t admitted = 0;
    printf("Orbits (%zu):\n", orbits);
    for (size_t i = 0; i < orbits; ++i) {
        RemanenceReconstructedOrbit orbit;
        if (!remanence_reconstruction_orbit(plan, i, &orbit)) {
            continue;
        }
        admitted += orbit.admitted ? 1 : 0;
        if (i < 3 || i + 1 == orbits) {
            printf("  step %" PRIu64 " at %" PRIu64 " um: %" PRIu32
                   " revolutions, spread %" PRIu32 " permille, %" PRIu64
                   " points%s\n",
                   orbit.position, orbit.radius_microns, orbit.revolutions,
                   orbit.count_spread_permille, orbit.points,
                   orbit.admitted ? "" : " (not admitted)");
        } else if (i == 3) {
            printf("  ...\n");
        }
    }
    printf("Admitted: %zu of %zu\n", admitted, orbits);

    size_t losses = remanence_reconstruction_declared_loss_count(plan);
    printf("Not carried (%zu):%s\n", losses, losses == 0 ? " nothing" : "");
    for (size_t i = 0; i < losses; ++i) {
        printf("  %s: %s (%" PRIu64 ")\n",
               remanence_reconstruction_declared_loss_code(plan, i),
               remanence_reconstruction_declared_loss_detail(plan, i),
               remanence_reconstruction_declared_loss_amount(plan, i));
    }
    size_t evidence = remanence_reconstruction_evidence_count(plan);
    printf("Known by (%zu):\n", evidence);
    for (size_t i = 0; i < evidence; ++i) {
        printf("  %s\n", remanence_reconstruction_evidence(plan, i));
    }

    /* Executing consumes the plan handle: it is freed either way and
     * must not be used again. */
    RemanenceImage *image = remanence_reconstruction_plan_execute(
        plan, 1024 * 1024, &error_category, &error, &error_rule);
    if (image == NULL) {
        report_error("error", error_category, error, error_rule);
        remanence_capture_set_free(set);
        return EXIT_FAILURE;
    }
    printf("Image:   %zu orbits, %" PRIu64 " bytes backed, %" PRIu64 " resident\n",
           remanence_image_orbit_count(image), remanence_image_backing_bytes(image),
           remanence_image_resident_bytes(image));

    remanence_image_free(image);
    remanence_capture_set_free(set);
    return EXIT_SUCCESS;
}

/* Writes all three C64 renditions off one remanence artifact. Each is a
 * P29 destination and each states its own loss; an existing destination
 * refuses by name rather than being overwritten. */
static int write_renditions(const char *path, const char *stem) {
    RemanenceErrorCategory error_category;
    char *error = NULL;
    char *error_rule = NULL;
    RemanenceImage *image =
        remanence_image_open(path, &error_category, &error, &error_rule);
    if (image == NULL) {
        report_error("error", error_category, error, error_rule);
        return EXIT_FAILURE;
    }

    char destination[1024];
    int status = EXIT_SUCCESS;

    snprintf(destination, sizeof destination, "%s.d64", stem);
    RemanenceD64Report *d64 = remanence_image_write_d64(image, destination, &error_category,
                                                        &error, &error_rule);
    if (d64 == NULL) {
        report_error("d64", error_category, error, error_rule);
        status = EXIT_FAILURE;
    } else {
        printf("Wrote:   %s (%" PRIu64 " bytes, %" PRIu32 " of %" PRIu32 " blocks)\n",
               remanence_d64_report_path(d64), remanence_d64_report_artifact_bytes(d64),
               remanence_d64_report_blocks_read(d64),
               remanence_d64_report_blocks_defined(d64));
        print_d64_loss(d64);
        remanence_d64_report_free(d64);
    }

    snprintf(destination, sizeof destination, "%s.g64", stem);
    RemanenceG64Report *g64 = remanence_image_write_g64(image, destination, &error_category,
                                                        &error, &error_rule);
    if (g64 == NULL) {
        report_error("g64", error_category, error, error_rule);
        status = EXIT_FAILURE;
    } else {
        printf("Wrote:   %s (%" PRIu64 " bytes, %zu half-tracks)\n",
               remanence_g64_report_path(g64), remanence_g64_report_artifact_bytes(g64),
               remanence_g64_report_half_track_count(g64));
        print_g64_loss(g64);
        remanence_g64_report_free(g64);
    }

    snprintf(destination, sizeof destination, "%s.p64", stem);
    RemanenceP64Report *p64 = remanence_image_write_p64(image, destination, &error_category,
                                                        &error, &error_rule);
    if (p64 == NULL) {
        report_error("p64", error_category, error, error_rule);
        status = EXIT_FAILURE;
    } else {
        printf("Wrote:   %s (%zu half-tracks)\n", destination,
               remanence_p64_half_track_count(NULL, p64));
        remanence_p64_report_free(p64);
    }

    remanence_image_free(image);
    return status;
}

int main(int argc, char **argv) {
    if (argc == 3 && strcmp(argv[1], "--list") == 0) {
        return list_archive(argv[2]);
    }
    if (argc == 3 && strcmp(argv[1], "--discover") == 0) {
        return show_discovery(argv[2]);
    }
    if ((argc == 3 || argc == 4) && strcmp(argv[1], "--remanence") == 0) {
        return show_remanence_image(argv[2], argc == 4 ? argv[3] : NULL);
    }
    if (argc == 4 && strcmp(argv[1], "--renditions") == 0) {
        return write_renditions(argv[2], argv[3]);
    }
    if ((argc == 3 || argc == 4) && strcmp(argv[1], "--reconstruct") == 0) {
        return reconstruct_capture(argv[2], argc == 4 ? strtoul(argv[3], NULL, 10) : 0);
    }
    if (argc == 2 && strcmp(argv[1], "--devices") == 0) {
        list_devices();
        return EXIT_SUCCESS;
    }
    if (argc < 2 || argc > 3) {
        fprintf(stderr, "Usage: %s <path-to-image> [device-type]\n", argv[0]);
        fprintf(stderr, "       %s --discover <path-to-image>\n", argv[0]);
        fprintf(stderr, "       %s --list <path-to-archive>\n", argv[0]);
        fprintf(stderr, "       %s --remanence <path-to-artifact> [write-to]\n", argv[0]);
        fprintf(stderr, "       %s --renditions <path-to-artifact> <destination-stem>\n",
                argv[0]);
        fprintf(stderr, "       %s --reconstruct <path-to-capture> [side]\n", argv[0]);
        fprintf(stderr, "       %s --devices\n", argv[0]);
        return EXIT_FAILURE;
    }
    /* Which drive serves a medium is machine configuration, so it is this
     * caller's to state -- and an `.h8d` wants `h17`. Told none, this
     * example asks the artifact instead. */
    const char *device_type = argc == 3 ? argv[2] : NULL;

    RemanenceErrorCategory error_category;
    char *error = NULL;
    char *error_rule = NULL;
    RemanenceSession *session = remanence_session_new();
    RemanenceDevice *device = NULL;
    RemanenceMedium *medium = NULL;
    if (device_type != NULL) {
        /* The acts: declare what the artifact is over this caller's own
         * open file (whoever opens owns the lock), add the drive to the
         * session's anonymous machine, and link them. Both handles are
         * borrowed -- the session owns them, so we never free them. */
        intptr_t source = open_source(argv[1]);
        if (source == 0) {
            fprintf(stderr, "error: cannot open '%s'\n", argv[1]);
            remanence_session_free(session);
            return EXIT_FAILURE;
        }
        /* The declaration carries both halves: what the artifact is,
         * and what recorded it -- which is the drive this caller just
         * named, since the medium goes into it. */
        const char *format = declared_format(argv[1]);
        medium = remanence_session_load_media(session, source, format, device_type,
                                              declared_block_bytes(format),
                                              &error_category, &error, &error_rule);
        if (medium == NULL) {
            report_error("error", error_category, error, error_rule);
            remanence_session_free(session);
            return EXIT_FAILURE;
        }
        device = remanence_session_add_device(session, device_type, &error_category,
                                              &error, &error_rule);
        if (device == NULL) {
            report_error("error adding the device", error_category, error, error_rule);
            remanence_session_free(session);
            return EXIT_FAILURE;
        }
        if (!remanence_device_insert(device, remanence_medium_id(medium), &error_category,
                                     &error, &error_rule)) {
            report_error("error", error_category, error, error_rule);
            remanence_session_free(session);
            return EXIT_FAILURE;
        }
    } else {
        /* Told no drive, this example asks the artifact rather than
         * assuming one: the convenience adds a device of the type the
         * image format records, pools the medium and seats it. A format
         * that records several refuses here, naming the types to pass as
         * the second argument. Here the *library* opens, so P7's
         * mandatory denial applies in full. */
        device = remanence_session_add_device_for(session, argv[1],
                                                  REMANENCE_ACCESS_INTENT_READ,
                                                  &error_category, &error, &error_rule);
        if (device == NULL) {
            report_error("error", error_category, error, error_rule);
            remanence_session_free(session);
            return EXIT_FAILURE;
        }
        medium = remanence_device_medium(device);
    }
    printf("Device:  %s (%s)\n", remanence_device_attachment(device),
           remanence_device_slot(device));

    RemanenceIdentification *identification = remanence_medium_identify(medium);

    printf("Source:  %s\n", name_or(remanence_medium_path(medium)));
    printf("Image:   %s\n", name_or(remanence_medium_image_path(medium)));
    print_medium_format(medium);
    print_assurance(medium);
    printf("Modified: %s\n\n", remanence_identification_modified(identification) ? "yes" : "no");

    size_t layer_count = remanence_identification_layer_count(identification);
    printf("Layers (%zu):\n", layer_count);
    for (size_t i = 0; i < layer_count; ++i) {
        printf("  - [%s] %s \"%s\" (confidence %d)%s\n",
               layer_kind_name(remanence_layer_kind(identification, i)),
               remanence_layer_id(identification, i),
               remanence_layer_name(identification, i),
               (int)remanence_layer_confidence(identification, i),
               remanence_layer_known(identification, i) ? "" : " [unknown]");
        print_size(identification, i);
    }

    size_t evidence_count = remanence_identification_evidence_count(identification);
    if (evidence_count > 0) {
        printf("\nEvidence:\n");
        for (size_t i = 0; i < evidence_count; ++i) {
            printf("  * %s\n", remanence_identification_evidence(identification, i));
        }
    }

    print_partitions(medium);

    /* File access lives on one node, and the node is reached through the
     * partition that composes it. The pool was settled when the medium
     * loaded, so this asks rather than probes; a medium bearing no
     * namespace is a named absence here rather than a failure of the
     * identification above. */
    int status = EXIT_SUCCESS;
    RemanenceErrorCategory namespace_category = REMANENCE_ERROR_CATEGORY_UNSUPPORTED;
    char *namespace_error = NULL;
    char *namespace_rule = NULL;
    RemanenceSpace *filesystem =
        open_namespace(medium, &namespace_category, &namespace_error, &namespace_rule);
    if (filesystem == NULL) {
        printf("\nFiles:   ");
        if (namespace_error != NULL) {
            report_error("none reachable", namespace_category, namespace_error, namespace_rule);
        } else {
            /* Nothing refused: no partition of this medium bears a
             * namespace and none declared one, which is an answer. */
            printf("none -- no partition of this medium bears or declares one\n");
        }
    } else {
        remanence_string_free(namespace_error);
        remanence_string_free(namespace_rule);
        RemanenceEntryList *entries =
            remanence_filesystem_entries(filesystem, "", &error_category, &error, &error_rule);
        if (entries == NULL) {
            report_error("\nerror listing the root", error_category, error, error_rule);
            status = EXIT_FAILURE;
        } else {
            size_t entry_count = remanence_entry_count(entries);
            printf("\nFiles (%zu, %s):\n", entry_count, remanence_filesystem_kind(filesystem));
            for (size_t i = 0; i < entry_count; ++i) {
                printf("  %s\t%" PRIu64 " bytes%s", remanence_entry_name(entries, i),
                       remanence_entry_size_bytes(entries, i),
                       remanence_entry_kind(entries, i) == REMANENCE_ENTRY_KIND_DIRECTORY
                           ? "\t<dir>"
                           : "");
                /* Whatever the recognizing filesystem declares beyond
                 * name, kind and size, in its own spelling. */
                size_t facts = remanence_entry_declared_count(entries, i);
                for (size_t fact = 0; fact < facts; ++fact) {
                    printf("\t%s=%s", remanence_entry_declared_key(entries, i, fact),
                           remanence_entry_declared_value(entries, i, fact));
                }
                printf("\n");
            }
            remanence_entry_list_free(entries);
        }
        /* The same handle carries the addressable vantage: one node, two
         * ways in. This reaches what the namespace above does not name. */
        if (remanence_volume_is_addressable(filesystem)) {
            unsigned char head[16];
            if (remanence_volume_read_at(filesystem, 0, head, sizeof head, &error_category,
                                         &error, &error_rule)) {
                printf("\nVolume:  %" PRIu64 " bytes at %" PRIu64 ", first bytes",
                       remanence_volume_length_bytes(filesystem),
                       remanence_volume_start_bytes(filesystem));
                for (size_t i = 0; i < sizeof head; ++i) {
                    printf(" %02x", head[i]);
                }
                printf("\n");
            } else {
                report_error("\nerror reading the volume extent", error_category, error,
                             error_rule);
                status = EXIT_FAILURE;
            }
        }
        remanence_space_free(filesystem);
    }

    show_drive_letters(medium);

    remanence_identification_free(identification);
    remanence_session_free(session);
    return status;
}
