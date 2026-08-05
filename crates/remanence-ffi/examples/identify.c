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

#include <inttypes.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

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

/* The image container format the adapter recognized, and the version the
 * formats that declare one carry. A version accessor answers 0 for an
 * image of any other format, so each is read only under its own format. */
static void print_device_format(const RemanenceDevice *device) {
    RemanenceDiskFormat format;
    if (!remanence_device_format(device, &format)) {
        /* A medium that is no disk image -- an archive -- presents no
         * disk to state the format or the size of. */
        return;
    }
    switch (format) {
        case REMANENCE_DISK_FORMAT_QCOW2:
            printf("Format:  qcow2 (version %" PRIu32 ")\n",
                   remanence_device_qcow2_version(device));
            break;
        case REMANENCE_DISK_FORMAT_VDI:
            printf("Format:  vdi (version %" PRIu32 ".%" PRIu32 ")\n",
                   remanence_device_vdi_version_major(device),
                   remanence_device_vdi_version_minor(device));
            break;
        case REMANENCE_DISK_FORMAT_RAW:
            printf("Format:  raw\n");
            break;
    }
    printf("Size:    %" PRIu64 " bytes\n", remanence_device_size(device));
}

/* What the open established about the evidence beneath it (P28), and what
 * that narrows. A verified medium says so in one line; a degraded one
 * states the condition, the evidence, the extents that read, and the
 * access it actually has -- before anything is read from it. */
static void print_assurance(const RemanenceDevice *device) {
    RemanenceAssurance *assurance = remanence_device_assurance(device);
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

static const char *outcome_name(RemanenceLetterOutcome outcome) {
    switch (outcome) {
        case REMANENCE_LETTER_OUTCOME_VOLUME: return "volume";
        case REMANENCE_LETTER_OUTCOME_DECLARED_DEVICE: return "declared-device";
        case REMANENCE_LETTER_OUTCOME_PHANTOM: return "phantom";
        case REMANENCE_LETTER_OUTCOME_UNDETERMINED: return "undetermined";
    }
    return "undetermined";
}

/* Composes the drive letters a DOS machine holding this one device would
 * have presented. The machine facts are ours to assert — this device is the
 * first fixed device attached — and the assignment rule is the library's.
 * No variant is stated here, so a letter the claimed rules disagree on
 * comes back undetermined rather than guessed. */
static void show_drive_letters(RemanenceDevice *device) {
    RemanenceErrorCategory error_category;
    char *error = NULL;
    char *error_rule = NULL;

    RemanenceDiskReport *report =
        remanence_device_inspect(device, &error_category, &error, &error_rule);
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
 * own family, and its content is the namespace that device resolves
 * to -- so this is the same walk a disk's filesystem takes, with no
 * archive journey of its own. */
static int list_archive(const char *path) {
    RemanenceErrorCategory error_category;
    char *error = NULL;
    char *error_rule = NULL;

    RemanenceSession *session = remanence_session_new();
    RemanenceDevice *device = remanence_session_add_device(
        session, "archive-device", &error_category, &error, &error_rule);
    if (device == NULL) {
        report_error("error adding the archive slot", error_category, error, error_rule);
        remanence_session_free(session);
        return EXIT_FAILURE;
    }
    if (!remanence_device_load_media(device, path, REMANENCE_ACCESS_INTENT_READ,
                                     &error_category, &error, &error_rule)) {
        report_error("error", error_category, error, error_rule);
        remanence_session_free(session);
        return EXIT_FAILURE;
    }

    printf("Archive: %s\n", remanence_device_path(device));
    printf("Device:  %s (%s)\n", remanence_device_attachment(device),
           remanence_device_family(device));
    printf("Size:    %" PRIu64 " bytes\n\n", remanence_device_image_size_bytes(device));

    RemanenceSpace *namespace =
        remanence_device_filesystem(device, &error_category, &error, &error_rule);
    if (namespace == NULL) {
        report_error("error reaching the namespace", error_category, error, error_rule);
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

/* Lists the device families this release claims, so a caller can see what
 * a machine's slots may be. Interior names of the lineage classify and
 * instantiate nothing, and the listing says which is which. */
static void list_families(void) {
    size_t count = remanence_device_family_count();
    printf("Device families (%zu):\n", count);
    for (size_t i = 0; i < count; ++i) {
        const char *kind_of = remanence_device_family_kind_of(i);
        printf("  %-16s %s%s%s%s\n",
               remanence_device_family_id(i),
               remanence_device_family_name(i),
               kind_of == NULL ? "" : ", a kind of ",
               kind_of == NULL ? "" : kind_of,
               remanence_device_family_is_concrete(i) ? "" : " [classifies only]");
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
    printf("Medium:  %s (%s)\n", remanence_discovery_media_type_name(discovery),
           remanence_discovery_media_type(discovery));
    printf("Size:    %" PRIu64 " bytes\n", remanence_discovery_size(discovery));

    size_t families = remanence_discovery_device_family_count(discovery);
    printf("Drives served it (%zu):\n", families);
    for (size_t i = 0; i < families; ++i) {
        printf("  %s\n", remanence_discovery_device_family(discovery, i));
    }
    /* Two different questions: where the medium could go, above, and
     * where it came from, here. A format declaring no default is
     * ordinary -- a raw image says nothing about its machine. */
    const char *declared = remanence_discovery_default_device(discovery);
    printf("Declared by the format: %s\n", declared != NULL ? declared : "nothing");

    remanence_discovery_free(discovery);
    return EXIT_SUCCESS;
}

int main(int argc, char **argv) {
    if (argc == 3 && strcmp(argv[1], "--list") == 0) {
        return list_archive(argv[2]);
    }
    if (argc == 3 && strcmp(argv[1], "--discover") == 0) {
        return show_discovery(argv[2]);
    }
    if (argc == 2 && strcmp(argv[1], "--families") == 0) {
        list_families();
        return EXIT_SUCCESS;
    }
    if (argc < 2 || argc > 3) {
        fprintf(stderr, "Usage: %s <path-to-image> [device-family]\n", argv[0]);
        fprintf(stderr, "       %s --discover <path-to-image>\n", argv[0]);
        fprintf(stderr, "       %s --list <path-to-archive>\n", argv[0]);
        fprintf(stderr, "       %s --families\n", argv[0]);
        return EXIT_FAILURE;
    }
    /* Which drive serves a medium is machine configuration, so it is this
     * caller's to state -- and an `.h8d` wants `heathkit-h17`. Stated or
     * not, nothing is reachable except through a device (P32). */
    const char *family = argc == 3 ? argv[2] : NULL;

    RemanenceErrorCategory error_category;
    char *error = NULL;
    char *error_rule = NULL;
    RemanenceSession *session = remanence_session_new();
    RemanenceDevice *device = NULL;
    if (family != NULL) {
        /* The two acts: add the drive to the session's anonymous machine,
         * then load the medium into it. The device is borrowed -- the
         * session owns it, so we never free it -- and it is the one
         * handle for the slot and its medium alike. */
        device = remanence_session_add_device(session, family, &error_category, &error,
                                              &error_rule);
        if (device == NULL) {
            report_error("error adding the device", error_category, error, error_rule);
            remanence_session_free(session);
            return EXIT_FAILURE;
        }
        if (!remanence_device_load_media(device, argv[1], REMANENCE_ACCESS_INTENT_READ,
                                         &error_category, &error, &error_rule)) {
            report_error("error", error_category, error, error_rule);
            remanence_session_free(session);
            return EXIT_FAILURE;
        }
    } else {
        /* Told no drive, this example asks the artifact rather than
         * assuming one: the convenience adds a device of the family the
         * image format declares and loads the medium into it. A format
         * declaring none refuses here, naming the drives to pass as the
         * second argument. */
        device = remanence_session_add_device_for(session, argv[1],
                                                  REMANENCE_ACCESS_INTENT_READ,
                                                  &error_category, &error, &error_rule);
        if (device == NULL) {
            report_error("error", error_category, error, error_rule);
            remanence_session_free(session);
            return EXIT_FAILURE;
        }
    }
    printf("Device:  %s (%s)\n", remanence_device_attachment(device),
           remanence_device_family(device));

    RemanenceIdentification *identification = remanence_device_identify(device);

    printf("Source:  %s\n", remanence_device_path(device));
    printf("Image:   %s\n", remanence_device_image_path(device));
    print_device_format(device);
    print_assurance(device);
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

    /* File access lives on one node. The device is asked what it
     * *resolves* to, and the listing comes from the filesystem it
     * answers with; a medium bearing no namespace is a named absence
     * here rather than a failure of the identification above. */
    int status = EXIT_SUCCESS;
    RemanenceSpace *filesystem =
        remanence_device_filesystem(device, &error_category, &error, &error_rule);
    if (filesystem == NULL) {
        printf("\nFiles:   ");
        report_error("none reachable", error_category, error, error_rule);
    } else {
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

    show_drive_letters(device);

    remanence_identification_free(identification);
    remanence_session_free(session);
    return status;
}
