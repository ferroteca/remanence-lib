// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The Commodore 1541's bitstream-to-bytestream transition: its group
//! code, and where framing begins (F32, F76, P12, P23, P30, P33).
//!
//! The rung beneath this one is not the family's. Clocking pulses into
//! bit cells is the phase-locked channel every enrolled family declares
//! its numbers for, and it lives at
//! [`crate::flux::presentation`]; what is *here* is the part that
//! differs in kind between families and cannot be shared — the table
//! that says what a group of recorded bits means, and the landmark that
//! says where a byte begins.
//!
//! **The profile owns every rule this applies** (P30). The framing
//! landmark is the family's declared bit convention and the byte values
//! its declared group-code table, and the family enrols this transition
//! on its profile as behavior rather than as a declaration central code
//! would have to interpret (P12). Nothing above branches on which family
//! arrived.
//!
//! **It assigns nothing above a byte.** The bytestream is a circular,
//! track-relative byte sequence. No byte here is a header, a data field,
//! a sector or a file; the codec locates the family's alignment landmark
//! because framing has to begin somewhere the family declares, and
//! having located one it says nothing whatever about what follows it.

use crate::error::Result;
use crate::evidence::{LossAccount, Provenance};
use crate::flux::bytestream::{
    ByteOutcome, ByteRecord, BytestreamBuilder, BytestreamFact, BytestreamFactKind,
};
use crate::flux::capture::SessionBacking;
use crate::flux::drive_profile::{C1541, GroupCodec};
use crate::flux::presentation::{
    Bitstream, Bytestream, BytestreamLocation, BytestreamReport, refuse,
};

/// The profile this transition is declared by.
const PROFILE: &str = "c1541";

// ------------------------------------------------------- codec policy

/// Where byte framing begins.
// The non-default choices are the deferred policy-deviation
// surface (D29): the pipeline's seams admit them, and the delivered
// caller is the profile's own declaration.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AlignmentPolicy {
    /// At the family's declared landmark, and nowhere else: bits before
    /// the first one are stated as unframed rather than guessed into
    /// bytes.
    Landmark,
    /// At the circle's own origin as well, the caller declaring that the
    /// medium's start is a byte boundary. Landmarks still reframe.
    Origin,
}

/// What a group holding a pattern the family's table does not assign
/// becomes.
// The non-default choices are the deferred policy-deviation
// surface (D29): the pipeline's seams admit them, and the delivered
// caller is the profile's own declaration.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UnassignedSymbolPolicy {
    /// The materialization stops and names the location and the bit.
    Refuse,
    /// The group keeps its own bits, stated as unresolved, and is
    /// counted. There is no nearest entry in the table.
    DeclareLoss,
}

/// The complete declared policy for one bitstream-to-bytestream
/// transition — the profile's declared `codec_policy` in every
/// delivered journey, exactly as the channel policy is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct GcrCodecPolicy {
    pub(crate) alignment: AlignmentPolicy,
    pub(crate) unassigned_symbol: UnassignedSymbolPolicy,
}

// ---------------------------------------------------------- the codec

/// The family's declared transition, as the profile enrols it: the
/// policy is read off the profile rather than passed in, which is P30
/// reached through the type.
pub(crate) fn materialize_declared(bitstream: &Bitstream, cache_bytes: u64) -> Result<Bytestream> {
    materialize_bytestream(bitstream, C1541.presentation.codec_policy, cache_bytes)
}

/// Materializes an encoded bytestream from a hardware bitstream.
pub(crate) fn materialize_bytestream(
    bitstream: &Bitstream,
    policy: GcrCodecPolicy,
    cache_bytes: u64,
) -> Result<Bytestream> {
    let profile = &C1541;
    if bitstream.profile().id != profile.id {
        return Err(refuse(
            profile.id,
            format!(
                "the bitstream was clocked for the family '{}' and this codec is the \
                 '{}' group code; a recording is resolved by the table its own family \
                 declares",
                bitstream.profile().id,
                profile.id
            ),
        ));
    }
    let codec = &profile.presentation.codec;
    let symbols_per_byte = codec.symbols_per_byte().ok_or_else(|| {
        refuse(
            PROFILE,
            format!(
                "the family's group code records {} bits to a symbol, which do not divide a \
                 byte; there is no byte for it to state",
                codec.data_bits
            ),
        )
    })?;
    let group_bits = symbols_per_byte * codec.symbol_bits;
    let landmark = u64::from(profile.presentation.read_channel.alignment_one_bits);
    if landmark == 0 {
        return Err(refuse(
            PROFILE,
            "the family declares no alignment landmark, so byte framing has nowhere to \
             begin",
        ));
    }

    let inner = bitstream.inner();
    let described = describe_codec(codec, &policy, inner.provenance());
    let mut builder = BytestreamBuilder::new(
        profile.id,
        codec.id,
        described.clone(),
        SessionBacking::create()?,
    )?;
    let mut loss = LossAccount::new();
    let mut reported = Vec::new();

    for location in inner.locations() {
        let key = location.key();
        let (numerator, denominator) = key.position();
        // The location's bits, gathered a chunk at a time so that no
        // other location is touched on the way to this one's bytes.
        let mut bits: Vec<bool> = Vec::with_capacity(location.cells() as usize);
        let mut unrecorded = 0u64;
        for ordinal in 0..inner.cell_chunks(location) {
            for cell in inner.cell_chunk(location, ordinal)? {
                if !cell.evidence().is_recorded() {
                    unrecorded += 1;
                }
                bits.push(cell.one());
            }
        }
        let framed = Framed::resolve(&bits, landmark, policy.alignment);

        let mut records = Vec::new();
        let mut facts = Vec::new();
        let mut unassigned = 0u64;
        for (at, run) in &framed.landmarks {
            facts.push(BytestreamFact::new(
                BytestreamFactKind::Alignment {
                    at_bit: *at as u64,
                    run_bits: *run as u64,
                },
                Provenance::new(PROFILE).note(format!(
                    "a run of {run} one bits, which is the family's declared framing \
                     landmark; byte framing resumes after it and nothing is claimed \
                     about what follows"
                )),
            ));
        }
        for (start, end) in &framed.segments {
            let mut at = *start;
            while at + group_bits as usize <= *end {
                let mut value = 0u64;
                for offset in 0..group_bits as usize {
                    value = value << 1 | u64::from(bits[at + offset]);
                }
                let outcome = resolve_group(codec, symbols_per_byte, value);
                if outcome.is_none() {
                    if policy.unassigned_symbol == UnassignedSymbolPolicy::Refuse {
                        return Err(refuse(
                            PROFILE,
                            format!(
                                "the group at bit {at} of location {numerator}/{denominator} \
                                 holds a pattern the family's table does not assign, and the \
                                 policy refuses rather than reporting a value it never \
                                 recorded"
                            ),
                        ));
                    }
                    unassigned += 1;
                }
                records.push(ByteRecord::new(
                    at as u64,
                    outcome.map_or(
                        ByteOutcome::Unresolved { bits: value },
                        ByteOutcome::Resolved,
                    ),
                ));
                at += group_bits as usize;
            }
            if at < *end {
                facts.push(BytestreamFact::new(
                    BytestreamFactKind::Unframed {
                        at_bit: at as u64,
                        bits: (*end - at) as u64,
                    },
                    Provenance::new(PROFILE).note(
                        "bits no framed group covers, stated rather than padded into a \
                         byte",
                    ),
                ));
            }
        }
        for (at, span) in &framed.unframed {
            facts.push(BytestreamFact::new(
                BytestreamFactKind::Unframed {
                    at_bit: *at as u64,
                    bits: *span as u64,
                },
                Provenance::new(PROFILE).note(
                    "bits framing does not reach, no landmark having placed a byte \
                           boundary ahead of them",
                ),
            ));
        }

        let unframed_bits: u64 = facts
            .iter()
            .filter_map(|fact| match fact.kind() {
                BytestreamFactKind::Unframed { bits, .. } => Some(*bits),
                BytestreamFactKind::Alignment { .. } => None,
            })
            .sum();
        let landmark_bits: u64 = framed.landmarks.iter().map(|(_, run)| *run as u64).sum();
        if landmark_bits > 0 {
            loss.add(
                "alignment-landmark",
                "bits forming the family's framing landmark, which say where bytes \
                 begin rather than becoming one",
                landmark_bits,
            );
        }
        if unframed_bits > 0 {
            loss.add(
                "unframed-bits",
                "bits no framed group covers, which no byte carries",
                unframed_bits,
            );
        }
        if unassigned > 0 {
            loss.add(
                "unassigned-symbol",
                "groups holding a pattern the family's table does not assign, kept as \
                 their own bits because there is no nearest entry",
                unassigned,
            );
        }
        if unrecorded > 0 {
            loss.add(
                "resolved-bit-evidence",
                "bits a declared rule resolved rather than the medium recording them, \
                 whose rule a byte does not carry",
                unrecorded,
            );
        }

        reported.push(BytestreamLocation {
            half_track_numerator: numerator,
            half_track_denominator: denominator,
            surface: key.surface(),
            bytes: records.len() as u64,
            resolved_bytes: records.len() as u64 - unassigned,
            unassigned_groups: unassigned,
            alignments: framed.landmarks.len() as u64,
            longest_landmark_bits: framed
                .landmarks
                .iter()
                .map(|(_, run)| *run as u64)
                .max()
                .unwrap_or(0),
            unframed_bits,
        });

        builder.add_location(
            key.clone(),
            &records,
            &facts,
            Provenance::new(PROFILE).note(format!(
                "resolved from the location's {} clocked bits by the family's declared \
                 group code",
                bits.len()
            )),
        )?;
    }

    let (mut bytestream, sink, total) = builder.seal()?;
    bytestream.attach_backing(Box::new(sink.into_source()), total, cache_bytes);

    let report = BytestreamReport {
        profile_id: profile.id.to_owned(),
        codec_id: codec.id.to_owned(),
        codec_name: codec.name.to_owned(),
        symbol_bits: codec.symbol_bits,
        data_bits: codec.data_bits,
        symbols_per_byte,
        locations: reported,
        declared_loss: loss.into_entries(),
        evidence: described.notes,
    };
    Ok(Bytestream::new(bytestream, report, profile))
}

/// One group of bits as the table reads it, or nothing where it holds a
/// pattern the family does not assign.
fn resolve_group(codec: &GroupCodec, symbols_per_byte: u32, group: u64) -> Option<u8> {
    let mask = (1u64 << codec.symbol_bits) - 1;
    let mut value = 0u8;
    for at in 0..symbols_per_byte {
        let shift = codec.symbol_bits * (symbols_per_byte - 1 - at);
        let symbol = u16::try_from(group >> shift & mask).ok()?;
        value = value << codec.data_bits | codec.value_of(symbol)?;
    }
    Some(value)
}

/// Where framing begins, what it covers, and what it does not.
///
/// Framing does not cross the circle's origin, and that is a stated rule
/// rather than an oversight: the origin is where the medium's own
/// reduction placed the circle's start — for a 1541 the write splice —
/// which is the one angle at which the recording is genuinely
/// discontinuous. A landmark spanning it is therefore two runs, and a
/// caller who wants framing to begin at the origin declares it.
struct Framed {
    /// Landmark runs, as (bit, run length).
    landmarks: Vec<(usize, usize)>,
    /// Framed spans, half-open.
    segments: Vec<(usize, usize)>,
    /// Spans framing does not reach, as (bit, length).
    unframed: Vec<(usize, usize)>,
}

impl Framed {
    fn resolve(bits: &[bool], landmark: u64, alignment: AlignmentPolicy) -> Self {
        let length = bits.len();
        let mut landmarks = Vec::new();
        let mut segments = Vec::new();
        let mut unframed = Vec::new();

        // Every landmark-length run of ones.
        let mut at = 0usize;
        while at < length {
            if bits[at] {
                let start = at;
                while at < length && bits[at] {
                    at += 1;
                }
                if (at - start) as u64 >= landmark {
                    landmarks.push((start, at - start));
                }
            } else {
                at += 1;
            }
        }

        let mut cursor = 0usize;
        let mut before_the_first = true;
        for (start, run) in &landmarks {
            if *start > cursor {
                if before_the_first && alignment == AlignmentPolicy::Landmark {
                    unframed.push((cursor, *start - cursor));
                } else {
                    segments.push((cursor, *start));
                }
            }
            cursor = start + run;
            before_the_first = false;
        }
        if cursor < length {
            if before_the_first && alignment == AlignmentPolicy::Landmark {
                unframed.push((cursor, length - cursor));
            } else {
                segments.push((cursor, length));
            }
        }

        Self {
            landmarks,
            segments,
            unframed,
        }
    }
}

fn describe_codec(
    codec: &GroupCodec,
    policy: &GcrCodecPolicy,
    bitstream: &Provenance,
) -> Provenance {
    let mut provenance = Provenance::new(PROFILE)
        .note(format!("{}: {}", codec.name, codec.provenance))
        .note(format!(
            "{} bits of the recording carry {} bits of a byte",
            codec.symbol_bits, codec.data_bits
        ))
        .note(match policy.alignment {
            AlignmentPolicy::Landmark => {
                "byte framing begins at the family's declared landmark and nowhere else".to_owned()
            }
            AlignmentPolicy::Origin => {
                "byte framing begins at the circle's origin as well, the caller \
                 declaring it a byte boundary"
                    .to_owned()
            }
        })
        .note(match policy.unassigned_symbol {
            UnassignedSymbolPolicy::Refuse => {
                "a pattern the table does not assign stops the materialization".to_owned()
            }
            UnassignedSymbolPolicy::DeclareLoss => {
                "a pattern the table does not assign keeps its own bits and is counted".to_owned()
            }
        })
        .note(
            "no byte here is a header, a data field, a sector or a file, and no \
             landmark introduces one"
                .to_owned(),
        );
    for note in &bitstream.notes {
        provenance = provenance.note(format!("the bitstream beneath it: {note}"));
    }
    provenance
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCategory;
    use crate::flux::medium::LocationKey;
    use crate::flux::presentation::tests::{cell_of, medium_of, recorded};
    use crate::flux::presentation::{
        DensityPolicy, ReadChannelPolicy, UnzonedPolicy, WeakPulsePolicy, materialize_bitstream,
    };

    const CYCLES_PER_ROTATION: u64 = 3_200_000;

    fn channel_policy() -> ReadChannelPolicy {
        ReadChannelPolicy {
            density: DensityPolicy::Declared,
            unzoned: UnzonedPolicy::Omit,
            weak_pulse: WeakPulsePolicy::Seeded,
            seed: 0x0123_4567_89ab_cdef,
        }
    }

    fn codec_policy() -> GcrCodecPolicy {
        GcrCodecPolicy {
            alignment: AlignmentPolicy::Landmark,
            unassigned_symbol: UnassignedSymbolPolicy::DeclareLoss,
        }
    }

    #[test]
    fn a_declared_byte_sequence_survives_the_whole_journey_unchanged() {
        // The claim that matters: what the family's table records is what
        // the codec reads back, through the read channel, the backing and
        // the group code, with nothing in between guessing.
        //
        // The track is written full, because a half-written one would
        // test the codec against a blank tail rather than against the
        // recording.
        let location = LocationKey::new(C1541.id, 18, 0);
        let cell = cell_of(&location);
        let landmark = u64::from(C1541.presentation.read_channel.alignment_one_bits);
        let fits = ((CYCLES_PER_ROTATION / cell) - landmark) / 10;
        let mut written = vec![0x00u8, 0x52, 0xff, 0x0f, 0xa5, 0x7b];
        written.resize(fits as usize, 0x55);
        let medium = medium_of(location.clone(), &recorded(&written), &[]);

        let bitstream = materialize_bitstream(&medium, &C1541, channel_policy(), 1 << 20)
            .expect("the channel clocks it");
        let bytestream = materialize_bytestream(&bitstream, codec_policy(), 1 << 20)
            .expect("the codec resolves it");

        let inner = bytestream.inner();
        let held = inner.location(&location).expect("the location is held");
        let read: Vec<u8> = inner
            .bytes(held)
            .expect("the bytes read back")
            .iter()
            .filter_map(crate::flux::bytestream::ByteRecord::value)
            .collect();
        assert_eq!(read, written, "{} bytes read back", read.len());

        // And the landmark was located rather than assumed: one run of
        // ten ones, and framing began after it. Every group the family's
        // table assigns resolved, and the bits the circle had left over
        // are stated rather than padded into a byte.
        let location_report = &bytestream.inspect().locations[0];
        assert_eq!(location_report.alignments, 1);
        assert_eq!(location_report.longest_landmark_bits, 10);
        assert_eq!(location_report.unassigned_groups, 0);
        assert_eq!(location_report.bytes, written.len() as u64);
        assert!(location_report.unframed_bits < 10, "{location_report:?}");
    }

    #[test]
    fn a_pattern_the_table_does_not_assign_is_refused_or_kept_as_its_own_bits() {
        // Ten ones frame the stream, then a group of ten zeros, which is
        // two symbols the family assigns nothing to.
        let mut bits = vec![true; 10];
        bits.extend(std::iter::repeat_n(false, 10));
        bits.extend(recorded(&[0x12])[10..].iter().copied());
        let location = LocationKey::new(C1541.id, 18, 0);
        let medium = medium_of(location.clone(), &bits, &[]);
        let bitstream = materialize_bitstream(&medium, &C1541, channel_policy(), 1 << 20)
            .expect("the channel clocks it");

        let error = materialize_bytestream(
            &bitstream,
            GcrCodecPolicy {
                unassigned_symbol: UnassignedSymbolPolicy::Refuse,
                ..codec_policy()
            },
            1 << 20,
        )
        .expect_err("a pattern the table does not assign is refused");
        assert_eq!(error.category(), ErrorCategory::InvalidImage);
        assert!(error.to_string().contains("does not assign"), "{error}");

        let bytestream = materialize_bytestream(&bitstream, codec_policy(), 1 << 20)
            .expect("the other policy keeps it");
        assert!(bytestream.inspect().locations[0].unassigned_groups >= 1);
        assert!(
            bytestream
                .inspect()
                .declared_loss
                .iter()
                .any(|loss| loss.code == "unassigned-symbol")
        );
    }

    #[test]
    fn the_channel_and_the_medium_policy_both_travel_into_the_bytestream() {
        // The transition preserves what produced its source: the medium's
        // own selection is still readable two layers up.
        let medium = medium_of(LocationKey::new(C1541.id, 18, 0), &recorded(&[0x12]), &[]);
        let bitstream = materialize_bitstream(&medium, &C1541, channel_policy(), 1 << 20)
            .expect("the channel clocks it");
        let bytestream = materialize_bytestream(&bitstream, codec_policy(), 1 << 20)
            .expect("the codec resolves it");

        let evidence = &bytestream.inspect().evidence;
        assert!(
            evidence
                .iter()
                .any(|line| line.contains("Commodore group-coded recording")),
            "{evidence:?}"
        );
        assert!(
            evidence.iter().any(|line| line.contains("Commodore 1541")),
            "{evidence:?}"
        );
        assert!(
            evidence
                .iter()
                .any(|line| line.contains("selected observation 0")),
            "{evidence:?}"
        );
    }
}
