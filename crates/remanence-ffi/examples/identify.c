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

static const char *container_kind_name(RmnContainerKind kind) {
    switch (kind) {
        case RMN_CONTAINER_KIND_ARCHIVE: return "archive";
        case RMN_CONTAINER_KIND_IMAGE: return "image";
        case RMN_CONTAINER_KIND_PHYSICAL_MEDIA: return "physical-media";
        case RMN_CONTAINER_KIND_FILESYSTEM: return "filesystem";
        case RMN_CONTAINER_KIND_UNKNOWN: return "unknown";
    }
    return "unknown";
}

static void print_size(const RmnIdentification *identification, size_t index) {
    uint64_t current = 0;
    uint64_t expected = 0;
    printf("      size: ");
    if (rmn_container_current_bytes(identification, index, &current)) {
        printf("%" PRIu64 " bytes", current);
    } else {
        printf("unknown");
    }
    if (rmn_container_expected_bytes(identification, index, &expected)) {
        printf(" (expected %" PRIu64 ")", expected);
    }
    printf("\n");
}

int main(int argc, char **argv) {
    if (argc != 2) {
        fprintf(stderr, "Usage: %s <path-to-image>\n", argv[0]);
        return EXIT_FAILURE;
    }

    char *error = NULL;
    RmnSession *session = rmn_session_open(argv[1], &error);
    if (session == NULL) {
        fprintf(stderr, "error: %s\n", error != NULL ? error : "unknown");
        rmn_string_free(error);
        return EXIT_FAILURE;
    }

    RmnIdentification *identification = rmn_session_identify(session);

    printf("Source:  %s\n", rmn_session_path(session));
    printf("Image:   %s\n", rmn_session_image_path(session));
    printf("Modified: %s\n\n", rmn_identification_modified(identification) ? "yes" : "no");

    size_t container_count = rmn_identification_container_count(identification);
    printf("Containers (%zu):\n", container_count);
    int has_hdos = 0;
    for (size_t i = 0; i < container_count; ++i) {
        RmnContainerKind kind = rmn_container_kind(identification, i);
        const char *id = rmn_container_id(identification, i);
        printf("  - [%s] %s \"%s\" (confidence %d)%s\n", container_kind_name(kind), id,
               rmn_container_name(identification, i),
               (int)rmn_container_confidence(identification, i),
               rmn_container_known(identification, i) ? "" : " [unknown]");
        print_size(identification, i);
        if (kind == RMN_CONTAINER_KIND_FILESYSTEM && strcmp(id, "hdos") == 0) {
            has_hdos = 1;
        }
    }

    size_t evidence_count = rmn_identification_evidence_count(identification);
    if (evidence_count > 0) {
        printf("\nEvidence:\n");
        for (size_t i = 0; i < evidence_count; ++i) {
            printf("  * %s\n", rmn_identification_evidence(identification, i));
        }
    }

    int status = EXIT_SUCCESS;
    if (has_hdos) {
        RmnHdosFileList *files = rmn_session_list_hdos_files(session, &error);
        if (files == NULL) {
            fprintf(stderr, "\nerror listing HDOS files: %s\n",
                    error != NULL ? error : "unknown");
            rmn_string_free(error);
            status = EXIT_FAILURE;
        } else {
            size_t file_count = rmn_hdos_file_count(files);
            printf("\nFiles (%zu):\n", file_count);
            for (size_t i = 0; i < file_count; ++i) {
                printf("  %s\t%" PRIu32 " sectors\t%s\t%s\n",
                       rmn_hdos_file_display_name(files, i),
                       rmn_hdos_file_size_sectors(files, i),
                       rmn_hdos_file_modified_date(files, i),
                       rmn_hdos_file_flags(files, i));
            }
            rmn_hdos_file_list_free(files);
        }
    }

    rmn_identification_free(identification);
    rmn_session_free(session);
    return status;
}
