<!--
SPDX-FileCopyrightText: 2026 Paul Galbraith
SPDX-License-Identifier: GPL-3.0-only
-->

# Computer-tape representations

Design for [F23](../FEATURES.md#f23--computer-tape-media-and-read-only-presentations),
serving U21 and proposed P26 alongside pledged P12–P15, P19, P21, and P23.
This is proposed, not implementation approval. Public names remain delivery
surface design.

## Tape is an ordered object medium

```text
tape image -> tape active state -> inspection / record streaming
                               -> typed tape-drive presentation
record selection -> bounded byte view -> filesystem/container/child artifact
```

The active state owns every partition and object. Higher views cannot discard
marks, merge records without a declared rule, or keep a mutable peer.

## Recorded state

```text
TapeMedia
  family/profile facts and provenance
  ordered partitions
  source metadata, evidence, issues

TapePartition
  opaque composition-scoped identity
  evidenced position facts
  ordered tape objects
  end observations

TapeObject
  opaque composition-scoped identity
  order and evidenced position
  data record | filemark | setmark | family marker
  record length and bytes when present
  provenance, observations, issues
```

Order is known even when an absolute identifier is not. Sources lacking
positions do not acquire them by array convention. Missing objects remain
explicit gaps, so later objects are never silently renumbered.

Fixed and variable sizes belong to records or their partition contract. A
short record is not padded, a long record is not split, and a mark is not a
zero-length record. Checksums, errors, retries, conflicts, drive responses,
and resume events remain observations rather than active snapshots.

## Partitions and tape files remain tape concepts

A tape partition is a sequential address domain, not a P16 table entry. A
tape file is a mark-bounded run, not a P19 entry. Record streaming preserves
boundaries. A byte view exists only under explicit concatenation rules.

Higher interpreters may recognize filesystems or containers over that view.
A child image can use P25 artifact mapping. Failure to recognize anything
higher leaves the tape objects inspectable.

## Adapters normalize only what they know

- Aaru can preserve partitions, tape files, records, and device metadata.
- A record-oriented emulator format may preserve records and marks only.
- One host file per tape file may have lost boundaries and positions.
- A flat dump supplies bytes only, not inferred tape structure.

Adapters keep captured, declared, inferred, synthesized, ambiguous, invalid,
and absent facts distinct. Aaru is an interoperability target, not the family
definition. Its adapter is independently implemented from published format
information and project-owned fixtures; external source is not imported.

## Inspection and sequential reading

Inspection reports related media facts, partitions, objects, tape files,
evidence, issues, and presentations. Callers select opaque identities rather
than reconstructing them from indexes or guessed marks.

Streaming consumes objects in order with type and length intact. Seeking
exists only where the contract supports it. An image index is an optimization,
not random-access media semantics.

## Typed tape-drive presentation

P15 supplies the lifecycle; the family supplies a read-only command boundary
covering the next record or mark, rewind, supported spacing and locating,
position, status, completion, and refusal. SCSI SSC is not universal when a
machine has a different natural seam.

Position, direction, motion, buffering, continuation, and latency are
ephemeral. Reads do not mutate media by advancing the presentation.

## Proposed P23 amendment

Pledging F23 requires one coherent vision act:

1. pledge P26 and U21;
2. add `tape` to P23's exact vocabulary;
3. retain one active layer per independently mutable instance;
4. keep derived record, byte, filesystem, and container views non-active; and
5. keep flat sources at their honest seam absent evidence-bearing composition.

No decision entry is needed; the texts and moving commit record that act.

## Acceptance shape

- Fixed and variable records survive without padding, splitting, or merging.
- Marks, partitions, end observations, and damaged positions retain order.
- Two materially different image shapes feed one family interface.
- Uninterpreted objects remain inspectable.
- A bounded higher view does not turn tape into a disk or named entry.
- Inspection and drive presentation observe one tape-active state.
- Aaru-specific metadata stays adapter-owned.
- Unsupported structure produces evidence-bearing refusals.

## Deliberately absent

- Physical acquisition and recovery procedures.
- Analog tape and sampled magnetic or servo signals.
- A random-access whole-tape disk or synthetic geometry.
- Promotion of flat dumps into absent structure.
- Writable tape operations in the initial feature.
- A format support claim before its adapter is delivered and tested.
