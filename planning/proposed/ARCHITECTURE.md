<!--
SPDX-FileCopyrightText: 2026 Paul Galbraith
SPDX-License-Identifier: GPL-3.0-only
-->

# ARCHITECTURE (proposed)

> **Status:** proposed, not pledged. Nothing in this file is approved for
> implementation. Principle numbers record order of issue, not priority.
>
> Sections headed `P<n> amendment` are drafted changes to a principle the
> project already carries, pledged or in force. They keep that principle's
> number, consume none of their own, and fold into its text on delivery
> ([SURFACES.md](../SURFACES.md)).

## P14 amendment — an archive is a medium, and its family is virtual

In-force P14 claims two media families, flexible magnetic and
logical-block, both physical articles. This amendment adds a third kind
of family: **virtual** media, whose first member is the **archive** —
zip, 7z, tar and their kin — the independent recorded state P14's own
definition already describes, held by no drive and backed by no article.

An archive medium's profile carries no physical fact: no form factor, no
coercivity, no addressable unit. Its one family fact is its native
vantage — **namespace**, where every physical family's is a space. Its
format adapter loads and saves its state as any adapter does, and the
named-entry state it loads is the medium's recorded content, the mutable
truth a session holds — P23's row for it already says so.

Nothing else in P14 moves: three facts keep their three homes, the
catalog stays declarative, and a media type outside it still refuses by
name. What the amendment forecloses is treating the archive as anything
else: not a device, not a container node, not a filesystem on a phantom
volume — a medium, loaded into a virtual slot (P32 amendment), whose
content is walked through the one namespace node (P35).

## P19 amendment — the namespace converges, and the composer moves to P35

In-force P19 carries the file-access seam and, lodged inside it, the
namespace-mapping composer with its three constraints. This amendment
slims P19 to the claim its title makes and re-homes the composer in P35,
which owns the machine namespace the composer exists to serve.

What P19 keeps: one file-access interface however reached — a
volume-backed filesystem, an archive medium's own content, and the
machine-composed namespace all present it — with the layers, identities,
and evidence that produced each result retained; transparency when every
seam has exactly one supported answer, and explicit exposure or refusal
when it does not; honest absence — valid non-file content is never
called empty and never forced through the seam as pseudo-files; and the
rule that selecting a file yields a byte stream only independent P12
recognition can make more of.

What P19 loses: the "serialized-container adapter" provider form — an
archive is a medium (P14 amendment) whose grammar is a P12 adapter at
the namespace seam, not a fourth kind of thing — and the
namespace-mapping composer with its three constraints, which are P35's.
The pledged P19 amendment ("a file-bearing interpretation states the
scope of its claim") is untouched by this one and folds into the slimmed
text as written.

## P24 — Optical media has a family-owned active layer

An optical medium whose recorded structure is observable above a generic
logical-block device uses a family-owned **optical** durable active layer.
Its active representation is exactly one family-declared seam: a captured
signal representation when sampled channel or RF observations are the best
available evidence, or a recorded-program representation when the source
begins at decoded drive- or player-visible structure. The report names which
representation is active. Decoding a signal into a program view does not
create a second mutable durable peer.

For compact disc the recorded-program state can preserve ordered sessions,
tracks, indexes, gaps, lead-in and lead-out facts, disc-relative frames, track
modes, the 2,352-byte main channel, P–W subchannels, and per-field provenance.
A LaserDisc signal state can instead preserve time-indexed RF samples, sample
clock and capture provenance; its decoded program views can include video,
audio, vertical-blanking information, frame or chapter addressing, and mapped
digital data. Other optical families define their own applicable state at the
lowest useful evidenced seam. No one family's schema is imposed on DVD,
Blu-ray, magneto-optical, or later media whose observable structures differ.

Optical is neither block nor magnetic flux. At the signal seam it may claim
sampled channel or RF observations and their timebase, but never silently
promotes those measurements into original pits, lands, surface state, or
pickup physics. At the program seam it claims only the recorded units,
channels, addressing, and layout visible through the applicable drive or
player contract. Parallel channels and marker or layout facts are parts of
one optical active state, not additional active layers. Capture observations
such as retries, C2 reports, drive offset, RF capture chain, and conflicting
reads attach as evidence and never become deterministic recorded state merely
because an image format stores them.

A signal seam may hold more than one model behind its single active layer, as
the magnetic family already does. A sampled capture and the corrected signal a
player would have read are different objects: the second carries a timebase and
frame the first does not, supplied by a declared decoder policy rather than
found in the samples. D14's test governs the boundary unchanged — disagreement
across observations is a capture fact, a corrected reading is a medium fact,
and neither becomes the other unasked. A report names which model it speaks
about. This creates no second active representation and no second commit
target.

An optical-active disc may expose a derived block presentation only over a
family-declared recorded extent or channel mapping that defines logical user
data. The presentation declares its scope, block size, address mapping, and
the evidence-bearing derivation from optical state. A CD data track, a
Blu-ray logical data extent, and LV-ROM digital data carried in channels which
otherwise hold audio are different family mappings of this rule. Unmapped
audio, video, gaps, and optical-only regions have no block presentation. A
track or channel mapping does not thereby become a P16 partition, and a mixed
medium never becomes one whole geometry-opaque block device. Volumes,
filesystems, and P19 file containers may compose above an eligible block
extent while every other optical structure remains present.

A block-, sector-, or filesystem-authoritative source may enter optical only
through an explicit family composition which claims an optical profile and
mastering rules. The atomic **generate-optical** transition synthesizes the
most honest optical state those inputs permit, identifies every manufactured
layout, raw-frame, error-correction, gap, and subchannel fact as synthetic,
and refuses contradictory or incomplete rules rather than inventing
precision. An ISO can therefore remain block-active for ordinary data access
or become the source of a newly mastered synthetic data disc when optical
hardware service is explicitly requested. It can never recover absent audio,
protection, damage, original mastering, or subchannel evidence. A generic LBA
hard drive remains terminal and is never inferred to be optical from its
content.

Image formats remain P12 adapters at their recorded representation seams. A
compound CCD/IMG/SUB source, BIN/CUE plus a sparse subchannel overlay, raw
LaserDisc RF capture, decoded LaserDisc CHD, Aaru Image Format, or another
optical encoding can all materialize optical state at different seams and
fidelity. A decoded source does not imply recoverable RF; an RF source remains
re-decodable without pretending its samples are literal surface geometry.
Single-file packaging confers no higher truth, and multiple source files do
not make the image a P19 file container. Each adapter declares which facts
are captured, declared, decoded, synthesized, patched, ambiguous, invalid, or
absent; no conversion silently promotes one provenance class into another.

P15 projects this durable state through a typed optical hardware presentation
at the useful common drive- or player-visible seam. Depending on the family,
that seam may provide commands, tracks, sectors, audio, subchannels, video,
vertical-blanking data, frame or chapter addresses, or mapped digital data.
Playback and pickup position, seek continuation, CAV or CLV rotational
progress, controller continuation, and pending causal effects are ephemeral
hardware state. Writes through either the hardware presentation or a derived
higher view mutate the one optical active instance and remain subject to P2
and P13 representability at commit.

Pledging this principle requires amending P23's exact active-layer table with:

| Active layer | Durable session state | Claim |
|---|---|---|
| **optical** | one family-owned signal or recorded-program representation, with its timebase or recorded layout, units, channels, mappings, and provenance | no inferred pits, lands, surface state, pickup physics, firmware, or geometry-opaque whole-disc block claim |

P23's one-active-layer rule otherwise remains unchanged. Block and optical are
different active representations, not concurrently mutable peers. A derived
eligible block presentation over optical state does not make block active;
an ISO opened only as blocks does not make optical active.

## P26 — Computer tape has a family-owned active layer

A computer-tape capture uses exactly one durable active representation owned
by its media family. “Tape” does not imply one universal object schema. The
adapter selects only the representation its source actually records:

- a **signal representation** preserves a time base and ordered transitions,
  pulse intervals, samples, gaps, provenance, and issues; C64 TAP is in this
  class; or
- a **recorded-object representation** preserves ordered partitions, records,
  filemarks, setmarks, end observations, provenance, and issues where the
  source carries them; Aaru and record-oriented drive captures are in this
  class.

Neither representation is silently promoted into the other. Pulse intervals
are not records, records are not sampled signals, fixed-size records do not
make a disk, and a filename or container label supplies no missing fidelity.
Unreadable, truncated, conflicting, resumed, or inferred observations retain
their evidenced positions.

Decoders and media parsers derive higher interpretations over the active
state. A standard C64 KERNAL decoder can derive a P19 flat file container from
TAP pulses while those pulses remain active. A record selection can expose a
bounded byte view under explicit concatenation rules. Filesystems, file
containers, and child images never replace the tape evidence from which they
were derived.

T64 is a logical C64 file container, not a pulse or recorded-object tape
representation. It can be active at P19 without acquiring a tape layer.
Conversely, a custom-loader TAP remains an honest signal capture even when no
file container can be derived.

A future write journey must name the representation it changes and define a
separate generate-tape transition when logical contents are encoded as pulses
or records. It cannot mutate a derived file view and pretend the source
evidence was edited in place. Physical transport and drive emulation remain
outside P15 until a use case requires their runtime semantics.

Pledging this principle requires replacing P23's proposed tape row with:

| Active layer | Durable session state | Claim |
|---|---|---|
| **tape** | one family-owned signal or recorded-object representation, with exact ordering, provenance, and issues | captured tape evidence at its actual fidelity, not a universal record list, random-access disk, derived file container, transport mechanism, or firmware |

P23 otherwise remains unchanged. Derived signal decoding, record grouping,
byte, filesystem, and file-container views do not become active.

## P32 amendment — devices are added, media are loaded, and families form a lineage

Pledged P32's session is the machine scope, and the storage model names
it so: `Session` becomes `Machine`. P32's opening — "there is no
separate machine object" — stays true by the rename rather than being
contradicted by it; nothing appears above the renamed scope.

**The two acts get their verbs.** A device is **added** — machine
configuration, possible empty, the drive U22 letters whether or not a
disk is in it — and a medium is **loaded** into it, which is P14's own
verb for what a format adapter does to media state. Each verb returns
its noun. Attach-as-one-act becomes the device-first convenience,
`add_device(path)`, admissible because device creation is its stated
act: a second call plainly adds a second device.

**Discovery is first-class, and machine-free.** `discover_media(path)`
claims the artifact for the read, identifies it, and answers with a
report — the exact medium, the concrete device families that accept it,
and the declared default, each carrying its evidence (P4) — mutating
nothing (P2). It is a library-level function on no handle at all,
because it consults catalogs and evidence, never machine configuration;
the machine's one-step conveniences use it beneath them, and a caller
uses it directly as the question asked before deciding what to add.

**Discovery is consumable, because it is expensive and because the
claim must not lapse.** Discovering a flux capture parses streams and
probes drive profiles; that work is held in the discovery, and
`load_media` accepts a discovery as it accepts a path, consuming it —
the parsed state moves into the loaded medium and nothing is done
twice. This is P29's plan-and-execute shape one seam over: the plan
computes, the execution consumes the plan. The claim taken at discovery
is held until the discovery is consumed or dropped, so there is no
window between the question and the load in which the artifact could
change — P7 continuity, not merely economy. A discovery is a claim
scope, which is exactly what the storage model's handle rule says earns
a handle; dropped unconsumed, it releases its claim and discards its
work.

**The one-step conveniences are dual, and both are declaration, not
guess.** `machine.add_device(path)` returns the device;
`machine.load_media(path)` returns the medium; underneath each is
`discover_media` plus two declarations. Discovery recognizes the
artifact (P12) and reports the exact medium, the concrete device
families that accept it — derived by asking the families, which declare
the media they accept, D19's direction unchanged — and the **default
device the image format declares**, a recording-side fact the media
type cannot honestly hold: a ten-sector hard-sectored 5.25-inch disk is
the article of both a Heathkit H-17 and a North Star MDS, but an H8D
records a Heathkit disk. Each one-step call adds a fresh device of the
declared default family and loads into it — stated in the contract,
never a silent reuse of an existing slot — and a format that declares
no default, as a raw image declares nothing about its machine, refuses
by name toward the two explicit acts. A declaration nobody makes is a
refusal, not a guess (P3).

**Device families form a stated lineage.** A family entry is as
concrete as the machine fact it asserts — a Commodore 1541, not "some
floppy" — and the catalog states each entry's lineage: a Commodore 1541
is a CBM floppy drive where shared characteristics warrant the
grouping, which is a floppy drive. The lineage is data in the family
catalog, never a type hierarchy, mirroring P30's profile catalog one
seam down and P14's media-type catalog beside it. **Interior names
classify; only concrete entries instantiate.** A device added as "some
floppy" exists in no machine, declares nothing `load_media` could check
a medium against, and letters nothing U22 could reason over — vagueness
in a machine fact is the refusal the CHS/LBA split below already makes,
one rung up the lineage. What P32 already states is unchanged by this:
identification and file access need none of a concrete device's
mechanics — the device must be real, not exercised.

**CHS and LBA hard drives are separate partitionable families.** One
device type exposing both vantages would carry a CHS⇄LBA translation
inside it, and translation was a BIOS fact, not a disk fact: MBR
entries hold both coordinate kinds and disagree on any large disk.
Which addressing the machine used is caller-asserted configuration, the
class of fact P32 already owns.

**The archive occupies a virtual slot.** An archive medium (P14
amendment) loads into an archive-family device with no mechanism: the
receiver `load_media` requires, the attachment identity the machine
knows it by, and nothing more. Whether that slot is visible in the
attachment namespace or stays behind the report is an open question the
storage model design carries.

## P35 — The machine namespace composes filesystems under a consumed or derived mapping

A **machine namespace** (`MachineFilesystem`) is one navigable
namespace over a machine's several filesystems — drive letters, mount
trees — presenting the same file-access interface every filesystem
presents (P19), and adding exactly one thing of its own: the mapping
that names each child.

The mapping has two sources and a strict precedence. **Where the
installed system persists its mapping, it is consumed**: Windows drive
letters, a Unix fstab — read as evidence, never derived. **Where the
system persists nothing, the mapping is derived** — a DOS machine's
letters were assigned at boot by a rule over machine configuration, and
nothing on the disks records the result — under three constraints:

- **The rule is an enumerated claim (P3).** The composer names the rule
  it applied, claims the system variants it implements and refuses the
  rest by name, and reports a mapping the claimed variants disagree on
  as undetermined rather than settled by the more common rule.
- **Evidence outranks a rule.** A persisted mapping governs, and
  derivation is never a fallback for a persisted mapping that could not
  be read.
- **A derived mapping is not evidence.** The asserted machine facts and
  the applied rule travel with the result as provenance (P4), and
  whatever the rule cannot settle is undetermined at the granularity it
  failed to establish — never filled from position, size, order, label,
  or which volume happened to read cleanly.

The machine namespace is a view, never an instance (P23): its mutations
project into the filesystems that compose it, and it holds no mutable
truth of its own. The mapping's machine facts — slot, family,
attachment order — are the machine's own configuration (P32), read from
the device set where the claimed families live there.

Pledged U16 is the consumed case; in-force U22 is the derived case,
served today by the composer in-force P19 carries and this principle
re-homes.
