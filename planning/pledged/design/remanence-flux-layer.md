<!--
SPDX-FileCopyrightText: 2026 Paul Galbraith
SPDX-License-Identifier: GPL-3.0-only
-->

# The remanence flux layer

The design behind F65 and F66: the gap-first reconstruction that fills
the **remanence image**, and the renditions read off it. Pledged at the
owner's direction, 2026-08-09. The image itself and its `.remanence`
artifact have since been delivered, and the code is their norm.

**Provenance.** The model and the algorithms are ported from the
owner's own flux-capture research implementation (Java, private,
unpublished, owner-authored throughout), where every ruling below was
measured against real captures — the same Pinball Construction Set
KryoFlux capture this repository already holds as a fixture. The port
is a rewrite into this crate's own vocabulary and mechanisms, not a
translation; title to every line is the owner's, so nothing enters
that the project cannot own. The research repository's DESIGN.md
holds the measurement record behind each constant; this document
carries only what the implementer here needs.

## The strata

The flux family holds **two models of a disk, deliberately**, and
they are not rivals:

- The **remanence image** (`remanence_image.rs`, delivered) is the
  physical stratum: what the medium holds, stated as facts of the
  surfaces — radius, angle, magnetization, write geometry. It is fit
  to nothing, addressed by no drive's stepping, and carries no clock:
  a cell length is a property of a *recording*, recoverable from the
  image, never a field of it.
- The **flux medium** (`flux_medium.rs`, delivered) is the served
  stratum: what a drive reads — one circular pulse stream per
  family-addressed location, an exact rotational frame, per-pulse
  strength. It remains the layer the presentation ladder
  (bitstream → bytestream) and the P64 adapter's decode stand on.

The reduction (F65) runs capture → remanence image. Renditions (F66)
run remanence image → artifact, and the p64 rendition passes through
a served projection — one multiply per point — on its way to the
delivered encode path. The media-first fold (F59) later takes the
whole family under `load_media`; nothing here moves the surface
shape it pledges.

## The reduction, staged

Stages, each its own seam, each testable alone:

1. **Revolution extraction** — the delivered capture model already
   bounds circular observations at index pairs; each revolution's
   transitions normalise to divisions of its own index-to-index span.
2. **Alignment** — gap correspondence: candidate origins voted by
   sampled windows over the gap sequence, then a resynchronising walk
   (skip-one-either-side with a confirmation ladder) pairs
   transitions. Identity lives in gaps; angles only position.
3. **Lattice** — comb periodogram over the intervals finds the cell;
   per-`(prev, self, next)` context medians measure the reader's
   displacement (peak shift), split by stream parity for the
   read-channel's alternating offset; a parity fit recovers the one
   free bit. Bounds, step and window rescale together.
4. **Warp** — per-revolution least-squares fit of departure from
   consensus onto low harmonics of the revolution; five harmonics,
   three passes, chosen by holdout in the lineage. The warp is the
   *total* wander — capture spindle and writer spindle together;
   attribution was measured to be unreachable and is not attempted.
5. **Gap-first angles** — mean each aligned gap across revolutions,
   subtract the contextual displacement, classify to whole cells,
   keep an interval off-lattice only where it departs the lattice
   *and* is consistent across revolutions (the medium holds it),
   re-solve the implied cell so closure is exact, integrate from the
   origin.
6. **Coherence** — per transition: seen in enough revolutions, spread
   within tolerance. Runs of failures long enough become one
   `Unaligned` span point; sense re-lays after each span.
7. **Merge** — adjacent steps carrying the same recording (gap-domain
   window agreement over measured retention) group; each step keeps
   its own measured angles; a group is emitted only where the caller
   asserted a recording.

Declared constants adopted from the lineage's measurements (the
angular figures are stated against the 2²⁸ frame): coherence
tolerance ≈ 2000 reference ticks in divisions, minimum agreement 3/4
of revolutions, indeterminate run 32; correspondence tolerance ≈ 6
ticks, walk tolerance ≈ 24, pooling floor 0.99; recording
discriminator: count-spread fraction < 0.001. Each lands as a
declared fact with provenance, not arithmetic a capture justifies.

The survey's fact classes — **evidenced** (the stream's own info
block), **measured** (rotation, recorded positions, step spacing),
**assumed** (standard-cited write geometry: ISO 6596-1/ECMA-70's
330 µm plateau; 432 µm guard as the lineage's working figure) — map
onto this crate's provenance discipline directly and travel with the
image.

Analysis internals use `f64` where the lineage does (periodogram,
medians, warp normal equations); every *declared* fact and every
*stored* fact stays integer or exact rational. That boundary — floats
measure, integers state — is the lineage's own and is kept.

## The renditions

As F66 states them. The GCR sector reading (sync scan, 4-to-5 group
code, header/data checksums, wrap-tolerant, repair-free) is
crate-private machinery: the reconstruction uses it to anchor and to
survey, the d64 rendition uses it to fill blocks, and neither is the
F61 surface. The g64 zone figures and the p64 index bridge reuse the
profile's declarations (P30) rather than restating them.

## Deferred, deliberately

- The divergence sidecar (the reconstruction account as its own
  text artifact) — the account rides the in-memory report until a
  journey needs the file.
- Flip-side pooling and the flippy transform's fitted origin — the
  pipeline's seams admit the second capture group; the journey
  arrives with a flippy fixture.
- Sector-anchored angle merging and checksum-selected arcs — the
  anchoring licence is written into the design above; the machinery
  lands when the fixtures demand it.
- The unguided orchestration (survey → recognise → rebuild both
  orientations) — the declared tier lands first; unguided belongs
  beside the question tier's argument.
- The served projection as a general verb (remanence image → flux
  medium for the presentation ladder) — the p64 rendition carries
  the one projection F66 needs.
