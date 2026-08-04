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

/* Lists what an archive holds, without reading any entry's data. */
static int list_archive(const char *path) {
    RemanenceErrorCategory error_category;
    char *error = NULL;
    RemanenceArchive *archive = remanence_archive_open(path, &error_category, &error);
    if (archive == NULL) {
        fprintf(stderr, "error (category %d): %s\n",
                (int)error_category, error != NULL ? error : "unknown");
        remanence_string_free(error);
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
    /* Nothing is reachable except through a device (P32): attach the
     * medium to a session, then borrow the medium the device holds. */
    RemanenceSession *session = remanence_session_new();
    char *attachment = NULL;
    if (!remanence_session_attach(session, argv[1], REMANENCE_ACCESS_INTENT_READ,
                                  &attachment, &error_category, &error)) {
        fprintf(stderr, "error (category %d): %s\n",
                (int)error_category, error != NULL ? error : "unknown");
        remanence_string_free(error);
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
            remanence_disk_list_hdos_files(disk, &error_category, &error);
        if (files == NULL) {
            fprintf(stderr, "\nerror listing HDOS files (category %d): %s\n",
                    (int)error_category, error != NULL ? error : "unknown");
            remanence_string_free(error);
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

    remanence_identification_free(identification);
    remanence_string_free(attachment);
    remanence_session_free(session);
    return status;
}
