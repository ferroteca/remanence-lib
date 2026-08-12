<!--
SPDX-FileCopyrightText: 2026 Paul Galbraith
SPDX-License-Identifier: GPL-3.0-only
-->

# The question tier

**Status: proposed — argued but not pledged. Nothing is worked from
here.** This is the *extension* above the delivered discovery surface,
not its successor. That surface — `discover_media`, the consumable
`Discovery`, `load_discovery`, `add_device_for`, the format's recorded
device types — was
once to be removed from S1–S3 by the media-first storage model
([../../pledged/design/media-first-storage-model.md](../../pledged/design/media-first-storage-model.md));
D30 reversed that, because discovery and loading answer different
questions and a caller who does not yet know what an artifact is has no
format to declare. What stays on this side of the gate is everything
below that the delivered verb does *not* do — ranked verdicts, policy
templates, gated derivation chains — to be argued as one coherent thing
rather than shipped piecemeal.

## The one concept

`discover` and `recognize` are the same question at two seams — **"what
is this?"** — and the tier answers it the same way everywhere: with
**ranked verdicts**, never with a creation.

- A verdict names a concrete catalog entry (an article, a device type,
  a drive profile), carries its confidence and the evidence that produced it
  (P4), and is ranked against its competitors — never resolved by
  catalog order, never auto-picked on margin.
- A verdict carries a **policy template**: the entry's family-declared
  fields filled (each marked with its declaring catalog entry as
  provenance), and holes opened only where *this artifact* genuinely
  presents a choice no family convention can make (P29's caller-only
  decisions). Adopting the template is the caller's declaration act, so
  the no-silent-defaults rule survives intact.
- The question tier asks; the creation verbs create. A verdict feeds a
  declaration (`load_media`, `as_type`); it never becomes one.

## The convenience over it: declared derivation chains

An intelligent `discover` may follow a derivation chain — archive →
sole capture set → mastered disk — and pool every link with
`derived_from` provenance, returning the deepest medium reached. It is
admissible only step-by-step, each step passing all three gates:

1. **Sole content** — the container holds exactly one recognizable
   logical artifact;
2. **Sole claimant** — exactly one enrolled entry claims it;
3. **Hole-free template** — the claimant's policy template has no
   caller-only holes for this artifact.

Any gate fails and the chain stops there, honestly, returning the
shallower result with the verdicts and the stop reason attached. The
test, as everywhere: *admissible where it declares, refused where it
would guess.*

## What returns here from the delivered surface

`discover_media`, the consumable `Discovery` handle,
`load_discovery` and its bound-declaring sibling, `add_device_for`, and
the image-format's recorded-device declaration (`device_type`,
`device_types`) — the ask-first journey
whole. Their claim-continuity idea (one open, no window between the
question and the load) is kept as a requirement on this tier's design,
as is the constraint that distinguishes them: discovery holds the claim
and builds no cache, so a tier layered above it inherits a question verb
that creates nothing.
