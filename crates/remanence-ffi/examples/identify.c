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

static const char *container_kind_name(RemanenceContainerKind kind) {
    switch (kind) {
        case REMANENCE_CONTAINER_KIND_ARCHIVE: return "archive";
        case REMANENCE_CONTAINER_KIND_IMAGE: return "image";
        case REMANENCE_CONTAINER_KIND_PHYSICAL_MEDIA: return "physical-media";
        case REMANENCE_CONTAINER_KIND_FILESYSTEM: return "filesystem";
        case REMANENCE_CONTAINER_KIND_UNKNOWN: return "unknown";
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
    if (remanence_container_current_bytes(identification, index, &current)) {
        printf("%" PRIu64 " bytes", current);
    } else {
        printf("unknown");
    }
    if (remanence_container_expected_bytes(identification, index, &expected)) {
        printf(" (expected %" PRIu64 ")", expected);
    }
    printf("\n");
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

/* Composes the drive letters a DOS machine holding this one disk would
 * have presented. The machine facts are ours to assert — this disk is the
 * first fixed disk attached — and the assignment rule is the library's.
 * No variant is stated here, so a letter the claimed rules disagree on
 * comes back undetermined rather than guessed. */
static void show_drive_letters(RemanenceDisk *disk) {
    RemanenceErrorCategory error_category;
    char *error = NULL;
    char *error_rule = NULL;

    RemanenceDiskReport *report =
        remanence_disk_inspect(disk, &error_category, &error, &error_rule);
    if (report == NULL) {
        report_error("\nerror inspecting disk", error_category, error, error_rule);
        return;
    }

    RemanenceDosMachine *machine = remanence_dos_machine_new();
    if (!remanence_dos_machine_assert_fixed_disk(machine, 0, report, &error_category,
                                                 &error, &error_rule)) {
        report_error("\nerror asserting the machine", error_category, error, error_rule);
        remanence_dos_machine_free(machine);
        remanence_disk_report_free(report);
        return;
    }

    RemanenceDriveMap *map =
        remanence_dos_machine_compose(machine, NULL, &error_category, &error, &error_rule);
    if (map == NULL) {
        report_error("\nerror composing drive letters", error_category, error, error_rule);
        remanence_dos_machine_free(machine);
        remanence_disk_report_free(report);
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
    remanence_disk_report_free(report);
}

/* Lists what an archive holds, without reading any entry's data. */
static int list_archive(const char *path) {
    RemanenceErrorCategory error_category;
    char *error = NULL;
    char *error_rule = NULL;
    RemanenceArchive *archive =
        remanence_archive_open(path, &error_category, &error, &error_rule);
    if (archive == NULL) {
        report_error("error", error_category, error, error_rule);
        return EXIT_FAILURE;
    }

    printf("Archive: %s\n", remanence_archive_path(archive));
    printf("Format:  %s (%s)\n", remanence_archive_format_name(archive),
           remanence_archive_format_id(archive));
    printf("Size:    %" PRIu64 " bytes\n\n", remanence_archive_size_bytes(archive));

    size_t entry_count = remanence_archive_entry_count(archive);
    printf("Entries (%zu):\n", entry_count);
    for (size_t i = 0; i < entry_count; ++i) {
        uint64_t compressed = 0;
        printf("  %s%s\t%" PRIu64 " bytes",
               remanence_archive_entry_name(archive, i),
               remanence_archive_entry_is_dir(archive, i) ? "/" : "",
               remanence_archive_entry_uncompressed_size(archive, i));
        if (remanence_archive_entry_compressed_size(archive, i, &compressed)) {
            printf("\t(%" PRIu64 " packed)", compressed);
        }
        printf("\n");
    }

    remanence_archive_free(archive);
    return EXIT_SUCCESS;
}

int main(int argc, char **argv) {
    if (argc == 3 && strcmp(argv[1], "--list") == 0) {
        return list_archive(argv[2]);
    }
    if (argc != 2) {
        fprintf(stderr, "Usage: %s <path-to-image>\n", argv[0]);
        fprintf(stderr, "       %s --list <path-to-archive>\n", argv[0]);
        return EXIT_FAILURE;
    }

    RemanenceErrorCategory error_category;
    char *error = NULL;
    char *error_rule = NULL;
    /* Nothing is reachable except through a device (P32): attach the
     * medium to a session, then borrow the medium the device holds. */
    RemanenceSession *session = remanence_session_new();
    char *attachment = NULL;
    if (!remanence_session_attach(session, argv[1], REMANENCE_ACCESS_INTENT_READ,
                                  &attachment, &error_category, &error, &error_rule)) {
        report_error("error", error_category, error, error_rule);
        remanence_session_free(session);
        return EXIT_FAILURE;
    }

    /* Borrowed: the session owns this, so we never free it. */
    RemanenceDisk *disk = remanence_session_medium(session, attachment);
    if (disk == NULL) {
        fprintf(stderr, "no medium attached at %s\n", attachment);
        remanence_string_free(attachment);
        remanence_session_free(session);
        return EXIT_FAILURE;
    }
    printf("Device:  %s\n", attachment);

    RemanenceIdentification *identification = remanence_disk_identify(disk);

    printf("Source:  %s\n", remanence_disk_path(disk));
    printf("Image:   %s\n", remanence_disk_image_path(disk));
    printf("Modified: %s\n\n", remanence_identification_modified(identification) ? "yes" : "no");

    size_t container_count = remanence_identification_container_count(identification);
    printf("Containers (%zu):\n", container_count);
    int has_hdos = 0;
    for (size_t i = 0; i < container_count; ++i) {
        RemanenceContainerKind kind = remanence_container_kind(identification, i);
        const char *id = remanence_container_id(identification, i);
        printf("  - [%s] %s \"%s\" (confidence %d)%s\n", container_kind_name(kind), id,
               remanence_container_name(identification, i),
               (int)remanence_container_confidence(identification, i),
               remanence_container_known(identification, i) ? "" : " [unknown]");
        print_size(identification, i);
        if (kind == REMANENCE_CONTAINER_KIND_FILESYSTEM && strcmp(id, "hdos") == 0) {
            has_hdos = 1;
        }
    }

    size_t evidence_count = remanence_identification_evidence_count(identification);
    if (evidence_count > 0) {
        printf("\nEvidence:\n");
        for (size_t i = 0; i < evidence_count; ++i) {
            printf("  * %s\n", remanence_identification_evidence(identification, i));
        }
    }

    int status = EXIT_SUCCESS;
    if (has_hdos) {
        RemanenceHdosFileList *files =
            remanence_disk_list_hdos_files(disk, &error_category, &error, &error_rule);
        if (files == NULL) {
            report_error("\nerror listing HDOS files", error_category, error, error_rule);
            status = EXIT_FAILURE;
        } else {
            size_t file_count = remanence_hdos_file_count(files);
            printf("\nFiles (%zu):\n", file_count);
            for (size_t i = 0; i < file_count; ++i) {
                printf("  %s\t%" PRIu32 " sectors\t%s\t%s\n",
                       remanence_hdos_file_display_name(files, i),
                       remanence_hdos_file_size_sectors(files, i),
                       remanence_hdos_file_modified_date(files, i),
                       remanence_hdos_file_flags(files, i));
            }
            remanence_hdos_file_list_free(files);
        }
    }

    show_drive_letters(disk);

    remanence_identification_free(identification);
    remanence_string_free(attachment);
    remanence_session_free(session);
    return status;
}
