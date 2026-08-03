<!--
SPDX-FileCopyrightText: 2026 Paul Galbraith
SPDX-License-Identifier: GPL-3.0-only
-->

# P64 image-format adapter

> **Status:** pledged, not delivered. This design serves F34 and authorizes no implementation outside that feature.

F34 is an ordinary P12 image-format adapter for P64, claimed in both directions. It owns the container's identity, recognition and evidence, version and structure validation, refusals, decoding into `FluxLayer`, and encoding of a mastered medium into a new artifact. It owns no selection, reconciliation, or timing policy: those are F33's, and the adapter refuses rather than repairs what arrives outside its claim.

The exact container grammar — signature, chunk vocabulary, version field, integrity fields, half-track addressing, and the width and meaning of a stored pulse's position and strength — is the adapter's own claim, enumerated in the delivered module from the published format description under P1 and P3. It is stated there rather than restated here, so one statement of it exists. P8 governs the version: validated before anything else is touched, and a version or structural feature beyond the claim fails immediately naming what was found and what is supported.

## Decode

A recognized P64 decodes into `FluxLayer` as P22 requires: stored pulse position and strength survive into the layer with their source identity, and the adapter neither flattens them into one deterministic bitcell or byte stream nor invents evidence the container does not hold. The decoded layer's provenance records that it came from a P64, not from a capture. This is the direction U7 consumes.

## Encode

Encoding accepts a mastered medium from F33 and writes a new artifact. The adapter reports what it can carry so the plan's declared-loss account is complete before the write (P29); anything the mastered medium holds that the container cannot express is named there, and a medium the claim cannot encode is refused rather than approximated.

The write takes its own P7 claim on the destination. An existing destination path is a named refusal, never an overwrite. Under P6 nothing is written until every check has passed, and under P9 an interruption leaves a complete artifact or none.

## Conformance

Round trip: a fixture mastered by F33 encodes, reopens through this adapter's own decode, and presents the same half-tracks, pulse positions, and strengths. Encoding is deterministic, so the same mastered medium produces the same bytes.

## Outside the feature

GCR, synchronization, sectors, filesystems, and files; a public pulse or flux iterator; any other C64 disk format; and any hardware, drive, or read-channel behavior, which belong to their own seams.
