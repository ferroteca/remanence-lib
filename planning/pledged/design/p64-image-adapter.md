<!--
SPDX-FileCopyrightText: 2026 Paul Galbraith
SPDX-License-Identifier: GPL-3.0-only
-->

# P64 image-format adapter

> **Status:** delivered; the feature that carried it has been struck and its
> handle retired. This remains the written statement of the adapter —
> implemented in `crates/remanence/src/p64.rs`, reached through
> `MasteredMedium::describe_p64`, `MasteredMedium::write_p64` and
> `P64Image::open` on all three application surfaces. It states what the
> adapter owns rather than specifying a surface, and no normative
> specification of S1–S3 has shipped, so it stays here rather than moving
> out (D11).
>
> Four things the prose left open, which the code had to settle.
>
> **A medium at rest is a third derivation.** The flux-medium layer offered
> two — reduced from evidence under a declared policy, or synthesized
> downward — and "a medium at rest in its own container, never an
> instrument's observation" is neither: this library did not derive it, and
> the container records nothing about who did. `Derivation::Stored` names
> that, and the invariant it protects is untouched, none of the three being
> recovered evidence.
>
> **The strength map is a translation and not a scaling.** The family
> declares three states and the published description names three values —
> always triggers, almost never, never — and they are the same three facts,
> so the crossing is exact in both directions. A state outside the family's
> vocabulary is refused rather than placed somewhere plausible on the
> container's finer scale, and a stored value between the extremes reads as
> weak with the finer thing the container said declared as loss.
>
> **The account is two reports read in sequence rather than one merged one.**
> The profile's is complete before the medium exists; the adapter's is
> complete before the file does. Each states its own crossing in its own
> terms, which is what keeps neither inferring the other's answer — a single
> account would have to be assembled by whichever of them ran last.
>
> **The adapter is enrolled nowhere.** P12's "one mechanical enrollment" is
> the delivered image catalog's, whose adapters open a byte-addressed device;
> block and flux are disjoint families (P13), so a flux container is reached
> through its own type, as the delivered capture-set adapter is. That seam
> has no catalog because it has one entry, and inventing one to satisfy the
> word "enrollment" would be ceremony.

This design's adapter is an ordinary P12 image-format adapter for P64, claimed in both directions. P64's authoritative image layer is a flux medium (P22), so the adapter owns the container's identity, recognition and evidence, version and structure validation, refusals, decoding into `FluxMedium`, and encoding of a mastered `FluxMedium` into a new artifact. It owns no selection, reconciliation, or timing policy: those are the mastering profile's, and the adapter refuses rather than repairs what arrives outside its claim.

The exact container grammar — signature, chunk vocabulary, version field, integrity fields, half-track addressing, and the width and meaning of a stored pulse's position and strength — is the adapter's own claim, enumerated in the delivered module from the published format description under P1 and P3. It is stated there rather than restated here, so one statement of it exists. P8 governs the version: validated before anything else is touched, and a version or structural feature beyond the claim fails immediately naming what was found and what is supported.

## Decode

A recognized P64 decodes into `FluxMedium` as P22 requires: stored pulse position and strength survive into the layer with their source identity, and the adapter neither flattens them into one deterministic bitcell or byte stream nor invents evidence the container does not hold. The decoded medium's provenance records that it came from a P64, not from a capture - it is a medium at rest in its own container, never an instrument's observation. This is the direction U7 consumes.

## Encode

Encoding accepts a mastered medium from the delivered mastering profile and writes a new artifact. The adapter reports what it can carry so the plan's declared-loss account is complete before the write (P29); anything the mastered medium holds that the container cannot express is named there, and a medium the claim cannot encode is refused rather than approximated.

The write takes its own P7 claim on the destination. An existing destination path is a named refusal, never an overwrite. Under P6 nothing is written until every check has passed, and under P9 an interruption leaves a complete artifact or none.

## Conformance

Round trip, and it is a same-layer comparison because both ends are a `FluxMedium`: a mastered fixture encodes, reopens through this adapter's own decode, and presents the same half-tracks, pulse positions, and strengths. Encoding is deterministic, so the same mastered medium produces the same bytes.

## Outside the feature

GCR, synchronization, sectors, filesystems, and files; a public pulse or flux iterator; any other C64 disk format; and any hardware, drive, or read-channel behavior, which belong to their own seams.
