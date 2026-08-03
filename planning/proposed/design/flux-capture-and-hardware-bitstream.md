<!--
SPDX-FileCopyrightText: 2026 Paul Galbraith
SPDX-License-Identifier: GPL-3.0-only
-->

# Flux capture and hardware-bitstream design

> **Status:** proposed, not pledged. This design serves F28 and authorizes no implementation.

## Purpose

F28 gives raw magnetic capture formats one durable home and adds two durable family-specific states above it: hardware bitstream, then encoded bytestream. The split is deliberately below synchronization, sector recovery, and filesystem interpretation: it lets a programmed drive retain recording-level bytes without asking a capture adapter to decide what the recording means.

This design aligns with P22's timed flux floor and amends P23: hardware bitstream and encoded bytestream join flux as durable magnetic active layers. Each materialization becomes the session's durable mutable truth; a G64-style bitstream source may begin at the middle layer directly.

## Durable flux state

An SCP, A2R, or KryoFlux adapter decodes only its container and capture semantics into flux state. It preserves, where present:

- timed flux-transition observations, with their source resolution and declared timing basis;
- track and side identity, capture/revolution identity, and recorded order;
- index, hard-sector, and other sensor/marker channels parallel to flux;
- capture metadata, sampling conditions, and provenance; and
- absence, irregularity, weak or contradictory observations, and unsupported format features as evidence or named refusals.

A capture is not an assertion that one ideal revolution existed. Several recorded revolutions are parallel evidence, not duplicates to average or a license to choose the cleanest result. A media profile and hardware profile must say whether the drive observes a selected revolution, a repeatable sequence, a seeded variation, or another specified behavior. Without such a rule, the requested presentation is refused rather than normalized silently.

The flux state is circular and track-relative, as P22 requires. Index and hard-sector topology stay in their own channels. The session holds and caches this state under P27; it does not retain a whole capture merely because it was stored that way on disk.

## Hardware-bitstream layer

The hardware-bitstream layer belongs to a specific drive family. It holds the serial, clocked, circular track-relative bit state at the boundary the family actually makes observable. It is manufactured from selected flux plus the declared mechanics and read-channel configuration, or loaded directly from an authoritative bitstream image. It is durable session state with its own P27 backing and mutable write set, not a cache or a generic public intermediate representation.

For a Commodore 1541, the layer sits after flux-pulse detection and the family's timing/recovery behavior, but before sync recognition and before GCR decoding. A run of zero or one decisions in this layer therefore does not mean a sync mark, GCR symbol, byte, sector, or file. Those are separate, higher interpretations controlled by the drive firmware or another explicitly claimed presentation.

Other families need not share that exact cut. A family may have a different bitstream form, or none; no universal bitstream schema is introduced. The common point is directional: a family profile may materialize its bitstream from flux, and any further FM/MFM/GCR decoding is a higher interpretation rather than an alteration of this durable layer.

An open has exactly one active layer. Flux-to-bitstream materialization is atomic: once validation succeeds, the bitstream replaces flux as the active mutable state and carries provenance for its source observations and profile. A write lands in the active bitstream. Commit is permitted only when every change can project to the image's authoritative layer without unclaimed loss; a flux-authoritative source whose bitstream write cannot be mastered back to flux is read-only or requires explicit conversion. Any bitstream-to-flux mastering transition is explicit, provenance-bearing, and subject to the same loss rule.

## Encoded-bytestream layer

Encoded bytestream is the next durable layer. In the 1541 family, GCR decoding materializes the circular byte sequence from hardware bitstream, with the codec, byte phase, and source bitstream provenance retained. It does not assign the result to a sync run, block header, data field, sector, or file. Those interpretations are above the bytestream. No known disk image format is claimed to begin at this layer; the design leaves that possibility open without inventing a generic source dialect.

## Boundaries and non-goals


## CHS and filesystem above the bytestream

The complete magnetic-disk interpretation path is flux → hardware bitstream
→ encoded bytestream → CHS → filesystem. A declared family decoder performs
synchronization and interprets encoded bytes as headers, data fields, and
sectors to materialize CHS; this is not licensed merely by having bytes.
P18 then recognizes and exposes a filesystem above that CHS view. CHS remains
the durable active media layer; the filesystem is its higher derived seam.
- Capture adapters do not reconstruct sectors, declare an encoding, or erase source disagreement in order to make a convenient byte stream.
- Hardware bitstream and encoded bytestream are durable, but sync, headers, data fields, sectors, and files are higher interpretations.
- Flux and bitstream do not become generic S1–S3 iterator APIs. Public access remains at the named drive-hardware or higher presentation seam.
- SCP, A2R, and KryoFlux are capture-container commitments, not a promise that every variant, controller, or capture-device extension is supported.
- Nothing here changes P13's authoritative-layer rule: a capture-backed open may write only when changes can return to the original capture format without unclaimed loss; otherwise it is read-only or requires explicit conversion.

## Delivery shape

The ownership model may be pledged as one feature, but individual format adapters and drive-family paths are independently sized implementation work. The first adapter must demonstrate preservation of multiple revolutions and marker channels. The first bitstream adapter must demonstrate a G64-style track active directly, and the first hardware path must demonstrate that a 1541 observes pre-sync, pre-GCR-decoding bits materialized from flux rather than from a sector shortcut. Every delivered public opening or hardware presentation changes S1, S2, and S3 coherently.
