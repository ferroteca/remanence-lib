<!--
SPDX-FileCopyrightText: 2026 Paul Galbraith
SPDX-License-Identifier: GPL-3.0-only
-->

# C1541 hardware-bitstream and encoded-bytestream design

> **Status:** proposed, not pledged. This design serves F32 and authorizes no implementation.

F32 consumes a `FluxMedium` — the delivered flux-medium layer — through a declared C1541 drive profile. It alone owns the mechanical selection policy, read-channel timing/recovery behavior, and the resulting circular, track-relative hardware bitstream. Neither a catalog nor a capture adapter may supply those conclusions.

The hardware bitstream is pre-sync and pre-GCR decoding. It is not a GCR symbol, byte, sector, or file. A declared C1541 GCR codec may then materialize an encoded bytestream with its codec and bitstream provenance, still without assigning synchronization, headers, data fields, sectors, or files.

Medium-to-bitstream and bitstream-to-bytestream transitions are atomic P23 active-layer changes. Their selected observations, profile, and codec travel as provenance; returning to the medium is a separate explicit mastering operation governed by P13. The presentation must never normalize several capture passes into a preferred revolution without the drive profile naming that selection or variation rule.

The feature does not promise a generic bitstream API, sector recovery, an image adapter for G64, or support for another drive family.