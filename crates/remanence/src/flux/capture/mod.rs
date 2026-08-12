// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The private flux-capture model (F30): raw magnetic capture evidence,
//! held without deciding what a medium, drive, codec, or filesystem
//! makes of it.
//!
//! A capture is never an active layer. It is authoritative image state
//! read by inspection and by mastering, and a reduction under P29 turns
//! it into the flux medium a drive is served from. Nothing here averages
//! timings, deduplicates pulses, chooses a cleanest pass, or turns
//! several recorded passes into one ideal rotation.
//!
//! Every item is crate-private and has no consumer until the capture
//! adapters (F31) land.
#![allow(dead_code)]

mod records;
mod sections;
mod wire;

pub(crate) use records::{
    CaptureEnvelope, ForeignRecord, Marker, MarkerKind, MetadataRecord, ObservationId,
    SourceDescriptor, SourceId, SourceRange, Tick, TimeBase, TrackKey, greatest_common_divisor,
};
pub(crate) use sections::{
    ByteSink, ByteSource, CaptureRunId, CaptureRunSlice, LEAF_ENTRIES, ScopeId, SectionAddress,
    SectionCache, SectionKey, SectionKind, SectionLocation, SectionWriter, SessionBacking,
    locate_section,
};
pub(crate) use wire::{read_text, read_varint, write_text, write_varint};

use wire::{decode_markers, decode_transitions, split_markers, split_transitions};

use std::collections::BTreeMap;
use std::sync::Mutex;

use crate::error::{Error, Result};
use crate::evidence::{Issue, Provenance};

/// One circular, track-relative observation bounded out of a capture
/// run: a declared circumference, the transitions within it, and the
/// marker channels recorded alongside them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Observation {
    id: ObservationId,
    provenance: Provenance,
    source: CaptureRunSlice,
    /// This location's source-record order, assigned when a track
    /// admits it. Carried rather than derived from list position,
    /// because a section-addressable backing may load one observation
    /// without its neighbours.
    ordinal: u64,
    span: Tick,
    transitions: Vec<Tick>,
    markers: Vec<Marker>,
}

impl Observation {
    /// Bounds an observation, refusing evidence that contradicts itself.
    ///
    /// `source` names the capture format for the diagnostic, so a
    /// refusal reads in the source's own terms (P4, P6).
    pub(crate) fn new(
        source: &str,
        provenance: Provenance,
        slice: CaptureRunSlice,
        span: Tick,
        transitions: Vec<Tick>,
        markers: Vec<Marker>,
    ) -> Result<Self> {
        if span == 0 {
            return Err(Error::invalid_image(
                source,
                "observation declares a span of zero, which states no \
                 circumference to measure its transitions against",
            ));
        }
        let mut previous: Option<Tick> = None;
        for (ordinal, &position) in transitions.iter().enumerate() {
            if position >= span {
                return Err(Error::invalid_image(
                    source,
                    format!(
                        "observation transition {ordinal} at tick {position} \
                         is not below the declared span of {span}"
                    ),
                ));
            }
            if let Some(previous) = previous
                && position <= previous
            {
                return Err(Error::invalid_image(
                    source,
                    format!(
                        "observation transition {ordinal} at tick {position} \
                         does not advance past the preceding tick {previous}"
                    ),
                ));
            }
            previous = Some(position);
        }
        for (ordinal, marker) in markers.iter().enumerate() {
            if marker.position() >= span {
                return Err(Error::invalid_image(
                    source,
                    format!(
                        "observation marker {ordinal} at tick {} is not below \
                         the declared span of {span}",
                        marker.position()
                    ),
                ));
            }
        }
        Ok(Self {
            // Both are assigned when a capture admits it: neither is
            // knowable until the observation has a home.
            id: ObservationId(0),
            provenance,
            source: slice,
            ordinal: 0,
            span,
            transitions,
            markers,
        })
    }

    pub(crate) fn id(&self) -> ObservationId {
        self.id
    }

    /// How this observation came to be known.
    pub(crate) fn provenance(&self) -> &Provenance {
        &self.provenance
    }

    /// The run and range this was bounded from.
    pub(crate) fn source(&self) -> CaptureRunSlice {
        self.source
    }

    /// Its place in this location's source-record order. Not a rank:
    /// it says nothing about whether the observation is good, complete,
    /// or the one a drive should be served from.
    pub(crate) fn ordinal(&self) -> u64 {
        self.ordinal
    }

    pub(crate) fn span(&self) -> Tick {
        self.span
    }

    pub(crate) fn transitions(&self) -> &[Tick] {
        &self.transitions
    }

    pub(crate) fn markers(&self) -> &[Marker] {
        &self.markers
    }

    /// The cyclic interval from the last transition back to the first.
    ///
    /// The source's wrap is preserved by the span rather than by a
    /// duplicate boundary pulse, so this is derived and never stored.
    /// `None` when the observation recorded no transitions at all.
    pub(crate) fn wrap_interval(&self) -> Option<Tick> {
        let first = *self.transitions.first()?;
        let last = *self.transitions.last()?;
        Some(self.span - last + first)
    }
}

/// One source transfer, preserved in its actual recorded time order.
///
/// A run holds everything the container transferred, including the flux
/// recorded before its first index and after its last — evidence that
/// bounding into circular observations does not consume.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CaptureRun {
    ordinal: u64,
    provenance: Provenance,
    transitions: Vec<Tick>,
    markers: Vec<Marker>,
}

impl CaptureRun {
    pub(crate) fn new(
        source: &str,
        ordinal: u64,
        provenance: Provenance,
        transitions: Vec<Tick>,
        markers: Vec<Marker>,
    ) -> Result<Self> {
        let mut previous: Option<Tick> = None;
        for (index, &position) in transitions.iter().enumerate() {
            if let Some(previous) = previous
                && position <= previous
            {
                return Err(Error::invalid_image(
                    source,
                    format!(
                        "capture run transition {index} at tick {position} does not \
                         advance past the preceding tick {previous}, so the run does \
                         not state its recorded time order"
                    ),
                ));
            }
            previous = Some(position);
        }
        Ok(Self {
            ordinal,
            provenance,
            transitions,
            markers,
        })
    }

    pub(crate) fn ordinal(&self) -> u64 {
        self.ordinal
    }

    /// How the run itself came to be known — what the adapter declared
    /// about the transfer, not anything this layer concluded.
    pub(crate) fn provenance(&self) -> &Provenance {
        &self.provenance
    }

    pub(crate) fn transitions(&self) -> &[Tick] {
        &self.transitions
    }

    pub(crate) fn markers(&self) -> &[Marker] {
        &self.markers
    }

    /// Bounds the run's circular observations at its index markers.
    ///
    /// Each consecutive pair of indices bounds one half-open window
    /// `[start, end)`, re-based to its own origin. A run with fewer than
    /// two indices supplies none: it remains inspectable evidence that
    /// simply cannot state a circumference, and no period is invented
    /// for it.
    pub(crate) fn observations(&self, source: &str) -> Result<Vec<Observation>> {
        let indices: Vec<Tick> = self
            .markers
            .iter()
            .filter(|marker| marker.kind() == &MarkerKind::Index)
            .map(Marker::position)
            .collect();

        indices
            .windows(2)
            .map(|bounds| {
                let (start, end) = (bounds[0], bounds[1]);
                if end <= start {
                    return Err(Error::invalid_image(
                        source,
                        format!(
                            "capture run index at tick {end} does not advance past \
                             the preceding index at tick {start}"
                        ),
                    ));
                }
                let transitions = self
                    .transitions
                    .iter()
                    .filter(|&&position| position >= start && position < end)
                    .map(|&position| position - start)
                    .collect();
                let markers = self
                    .markers
                    .iter()
                    .filter(|marker| marker.position() >= start && marker.position() < end)
                    .map(|marker| {
                        Marker::new(
                            marker.position() - start,
                            marker.kind().clone(),
                            marker.provenance().clone(),
                        )
                        .with_payload(marker.payload().to_vec())
                    })
                    .collect();
                Observation::new(
                    source,
                    Provenance::new(self.provenance.source).note(format!(
                        "bounded from capture run {} between the index markers                          at ticks {start} and {end}",
                        self.ordinal
                    )),
                    CaptureRunSlice::new(self.ordinal, start, end),
                    end - start,
                    transitions,
                    markers,
                )
            })
            .collect()
    }
}

/// What an opened capture keeps resident for one capture run: its
/// identity, its shape, and how it came to be known — never its
/// payload.
///
/// The transitions and markers themselves live in the backing and load
/// one bounded section at a time, because a capture set is routinely a
/// hundred and sixty-eight members and holding every pulse of it
/// resident is the assumption P27 forbids.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CaptureRunEntry {
    id: CaptureRunId,
    ordinal: u64,
    source: SourceId,
    transitions: u64,
    markers: u64,
    indices: u64,
    /// The last transition's tick — the extent of what was recorded,
    /// not a circumference. A run states no period.
    extent: Tick,
    before_first_index: u64,
    after_last_index: u64,
    transition_chunks: u64,
    marker_chunks: u64,
    provenance: Provenance,
}

impl CaptureRunEntry {
    pub(crate) fn id(&self) -> CaptureRunId {
        self.id
    }

    pub(crate) fn ordinal(&self) -> u64 {
        self.ordinal
    }

    /// The artifact this run was transferred from.
    pub(crate) fn source(&self) -> SourceId {
        self.source
    }

    pub(crate) fn transitions(&self) -> u64 {
        self.transitions
    }

    pub(crate) fn markers(&self) -> u64 {
        self.markers
    }

    /// How many of those markers are index events — the count that
    /// says how many circular observations the run could bound.
    pub(crate) fn indices(&self) -> u64 {
        self.indices
    }

    pub(crate) fn extent(&self) -> Tick {
        self.extent
    }

    /// Transitions recorded before the first index and after the last:
    /// evidence bounding into circular observations does not consume,
    /// counted here so it is visibly retained rather than assumed.
    pub(crate) fn before_first_index(&self) -> u64 {
        self.before_first_index
    }

    pub(crate) fn after_last_index(&self) -> u64 {
        self.after_last_index
    }

    pub(crate) fn provenance(&self) -> &Provenance {
        &self.provenance
    }
}

/// What an opened capture keeps resident for one observation, on the
/// same terms as [`CaptureRunEntry`]: identity and shape, never the
/// pulses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ObservationEntry {
    id: ObservationId,
    ordinal: u64,
    span: Tick,
    source: CaptureRunSlice,
    transitions: u64,
    markers: u64,
    provenance: Provenance,
}

impl ObservationEntry {
    pub(crate) fn id(&self) -> ObservationId {
        self.id
    }

    /// Its place in this location's source-record order. Not a rank.
    pub(crate) fn ordinal(&self) -> u64 {
        self.ordinal
    }

    pub(crate) fn span(&self) -> Tick {
        self.span
    }

    /// The run and range this was bounded from.
    pub(crate) fn source(&self) -> CaptureRunSlice {
        self.source
    }

    pub(crate) fn transitions(&self) -> u64 {
        self.transitions
    }

    pub(crate) fn markers(&self) -> u64 {
        self.markers
    }

    pub(crate) fn provenance(&self) -> &Provenance {
        &self.provenance
    }
}

/// Everything a capture holds for one location.
///
/// A track that exists with no observations is not the same fact as a
/// track the capture never supplied: the first is a location the source
/// declared and had no usable capture of, the second is a location the
/// capture is silent about. The layer is sparse so the two stay apart,
/// and neither is repaired into the other.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Track {
    key: TrackKey,
    runs: Vec<CaptureRunEntry>,
    observations: Vec<ObservationEntry>,
    issues: Vec<Issue>,
}

impl Track {
    pub(crate) fn new(key: TrackKey) -> Self {
        Self {
            key,
            runs: Vec::new(),
            observations: Vec::new(),
            issues: Vec::new(),
        }
    }

    pub(crate) fn key(&self) -> &TrackKey {
        &self.key
    }

    /// The source transfers recorded at this location, in the order
    /// they were transferred.
    pub(crate) fn runs(&self) -> &[CaptureRunEntry] {
        &self.runs
    }

    pub(crate) fn observations(&self) -> &[ObservationEntry] {
        &self.observations
    }

    pub(crate) fn issues(&self) -> &[Issue] {
        &self.issues
    }

    /// Takes an observation the capture has already identified, giving
    /// it the next ordinal in this location's source-record order.
    fn push_observation(&mut self, id: ObservationId, observation: &Observation) {
        self.observations.push(ObservationEntry {
            id,
            ordinal: self.observations.len() as u64,
            span: observation.span,
            source: observation.source,
            transitions: observation.transitions.len() as u64,
            markers: observation.markers.len() as u64,
            provenance: observation.provenance.clone(),
        });
    }
}

/// Where an opened capture's sections are read back from, and the
/// bounded working set they are served through (P27).
struct CaptureBacking {
    bytes: Box<dyn ByteSource + Send + Sync>,
    total_bytes: u64,
    cache: Mutex<SectionCache<SectionKey>>,
    /// The namespaces this reader can place. An index naming another
    /// one is not the index this layer wrote.
    known: Vec<&'static str>,
}

impl std::fmt::Debug for CaptureBacking {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CaptureBacking")
            .field("total_bytes", &self.total_bytes)
            .finish()
    }
}

/// One opened capture: its declared timing basis and the locations it
/// supplied evidence for.
#[derive(Debug)]
pub(crate) struct FluxCapture {
    envelope: CaptureEnvelope,
    time_base: TimeBase,
    tracks: BTreeMap<TrackKey, Track>,
    next_observation_id: u64,
    next_run_id: u64,
    backing: Option<CaptureBacking>,
}

impl FluxCapture {
    pub(crate) fn new(time_base: TimeBase) -> Self {
        Self {
            envelope: CaptureEnvelope::new(),
            time_base,
            tracks: BTreeMap::new(),
            next_observation_id: 0,
            next_run_id: 0,
            backing: None,
        }
    }

    pub(crate) fn time_base(&self) -> TimeBase {
        self.time_base
    }

    /// Everything the source stated besides its flux.
    pub(crate) fn envelope(&self) -> &CaptureEnvelope {
        &self.envelope
    }

    pub(crate) fn envelope_mut(&mut self) -> &mut CaptureEnvelope {
        &mut self.envelope
    }

    /// Admits an observation at a location the capture has declared,
    /// issuing its capture-wide identity.
    ///
    /// A location the capture never supplied is refused rather than
    /// created on the way past, which would turn silence about a
    /// location into a declaration of it.
    pub(crate) fn admit_observation(
        &mut self,
        key: &TrackKey,
        observation: &Observation,
    ) -> Result<ObservationId> {
        let id = ObservationId(self.next_observation_id);
        let track = self.tracks.get_mut(key).ok_or_else(|| {
            Error::invalid_image(
                key.namespace,
                format!(
                    "capture has no location {:?} to admit an observation to",
                    key.position
                ),
            )
        })?;
        track.push_observation(id, observation);
        self.next_observation_id += 1;
        Ok(id)
    }

    pub(crate) fn insert_track(&mut self, track: Track) {
        self.tracks.insert(track.key().clone(), track);
    }

    pub(crate) fn track(&self, key: &TrackKey) -> Option<&Track> {
        self.tracks.get(key)
    }

    /// Every supplied location, in key order — which is the source's
    /// own addressing sorted, not the order the adapter happened to
    /// hand them over in.
    pub(crate) fn tracks(&self) -> impl Iterator<Item = &Track> {
        self.tracks.values()
    }

    fn backing(&self, namespace: &'static str) -> Result<&CaptureBacking> {
        self.backing.as_ref().ok_or_else(|| {
            Error::invalid_image(
                namespace,
                "capture has no backing to read its evidence from, so nothing \
                 was ever written for it to serve",
            )
        })
    }

    /// Reads one section through the bounded working set.
    fn section(&self, key: &SectionKey) -> Result<Vec<u8>> {
        let backing = self.backing(key.track.namespace)?;
        let mut cache = backing
            .cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        cache
            .section(
                backing.bytes.as_ref(),
                backing.total_bytes,
                key,
                &backing.known,
            )
            .map(<[u8]>::to_vec)
    }

    /// Whether a section is currently in the working set. The layer's
    /// own bounded-reload test asks; nothing else does.
    fn holds_section(&self, key: &SectionKey) -> bool {
        self.backing.as_ref().is_some_and(|backing| {
            backing
                .cache
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .holds(key)
        })
    }

    /// One run's transitions, read back from the backing a chunk at a
    /// time. Never the whole capture, and never another location.
    pub(crate) fn run_transitions(
        &self,
        key: &TrackKey,
        run: &CaptureRunEntry,
    ) -> Result<Vec<Tick>> {
        let mut ticks = Vec::with_capacity(run.transitions as usize);
        for ordinal in 0..run.transition_chunks {
            let section = SectionKey::new(
                key.clone(),
                ScopeId::Run(run.id),
                SectionKind::TransitionChunk,
                ordinal,
            );
            ticks.extend(decode_transitions(&self.section(&section)?)?);
        }
        Ok(ticks)
    }

    /// One run's marker channels, in the order they were recorded.
    pub(crate) fn run_markers(&self, key: &TrackKey, run: &CaptureRunEntry) -> Result<Vec<Marker>> {
        let backing = self.backing(key.namespace)?;
        let known = backing.known.clone();
        let mut markers = Vec::with_capacity(run.markers as usize);
        for ordinal in 0..run.marker_chunks {
            let section = SectionKey::new(
                key.clone(),
                ScopeId::Run(run.id),
                SectionKind::MarkerChunk,
                ordinal,
            );
            markers.extend(decode_markers(
                key.namespace,
                &self.section(&section)?,
                &known,
            )?);
        }
        Ok(markers)
    }

    /// One observation, rebuilt from the run it was bounded from.
    ///
    /// The observation's payload shares the run's indexed chunks rather
    /// than duplicating them, so this reads the run's evidence over the
    /// recorded range and rebases it to the observation's own origin —
    /// the same cut, made again from the same bytes.
    pub(crate) fn observation(
        &self,
        key: &TrackKey,
        entry: &ObservationEntry,
    ) -> Result<Observation> {
        let run = self
            .track(key)
            .and_then(|track| {
                track
                    .runs
                    .iter()
                    .find(|run| run.ordinal == entry.source.run_ordinal())
            })
            .ok_or_else(|| {
                Error::invalid_image(
                    key.namespace,
                    format!(
                        "observation was bounded from capture run {}, which this \
                         location does not hold",
                        entry.source.run_ordinal()
                    ),
                )
            })?;
        let (start, end) = (entry.source.start(), entry.source.end());
        let transitions = self
            .run_transitions(key, run)?
            .into_iter()
            .filter(|position| *position >= start && *position < end)
            .map(|position| position - start)
            .collect();
        let markers = self
            .run_markers(key, run)?
            .into_iter()
            .filter(|marker| marker.position() >= start && marker.position() < end)
            .map(|marker| {
                Marker::new(
                    marker.position() - start,
                    marker.kind().clone(),
                    marker.provenance().clone(),
                )
                .with_payload(marker.payload().to_vec())
            })
            .collect();
        let mut observation = Observation::new(
            key.namespace,
            entry.provenance.clone(),
            entry.source,
            entry.span,
            transitions,
            markers,
        )?;
        observation.id = entry.id;
        observation.ordinal = entry.ordinal;
        Ok(observation)
    }
}

/// How many transitions or markers one chunk of the backing holds.
///
/// A record count rather than a byte count, so the same evidence always
/// splits the same way however the backing is rebuilt.
pub(crate) const CHUNK_RECORDS: usize = 4096;

/// Builds an opened capture and its backing together: the payload
/// streams into the sink as each location arrives, and only identity
/// and shape stay resident.
///
/// Locations must arrive in ascending key order, which is what the
/// section index requires and what makes a half-written backing
/// detectable rather than a layer with holes in it.
pub(crate) struct CaptureBuilder<S: ByteSink> {
    writer: SectionWriter<SectionKey, S>,
    capture: FluxCapture,
    chunk_records: usize,
}

impl<S: ByteSink> CaptureBuilder<S> {
    pub(crate) fn new(time_base: TimeBase, sink: S) -> Self {
        Self {
            writer: SectionWriter::to_sink(sink, LEAF_ENTRIES),
            capture: FluxCapture::new(time_base),
            chunk_records: CHUNK_RECORDS,
        }
    }

    /// A builder whose chunks hold at most `records` each. The layer's
    /// own tests use a tiny value to force several chunks out of a
    /// handful of transitions.
    pub(crate) fn with_chunk_records(mut self, records: usize) -> Self {
        self.chunk_records = records;
        self
    }

    pub(crate) fn envelope_mut(&mut self) -> &mut CaptureEnvelope {
        self.capture.envelope_mut()
    }

    /// Adds one location and everything captured at it: its transfers,
    /// the circular observations they bound, and whatever was qualified
    /// about either.
    pub(crate) fn add_location(
        &mut self,
        key: TrackKey,
        source: SourceId,
        runs: &[CaptureRun],
        issues: Vec<Issue>,
    ) -> Result<()> {
        let namespace = key.namespace;
        let mut track = Track::new(key.clone());
        track.issues = issues;
        self.writer.append(
            SectionKey::new(key.clone(), ScopeId::Track, SectionKind::TrackMetadata, 0),
            encode_track_metadata(runs.len() as u64, &track.issues),
        )?;
        if !track.issues.is_empty() {
            self.writer.append(
                SectionKey::new(key.clone(), ScopeId::Track, SectionKind::IssueChunk, 0),
                encode_issues(&track.issues),
            )?;
        }

        // Every run's sections before any observation's: a run scope
        // sorts ahead of an observation scope, so interleaving them
        // would emit the backing out of key order.
        for run in runs {
            let id = CaptureRunId(self.capture.next_run_id);
            self.capture.next_run_id += 1;
            let transitions = split_transitions(run.transitions(), self.chunk_records)?;
            let markers = split_markers(run.markers(), self.chunk_records);
            let indices: Vec<Tick> = run
                .markers()
                .iter()
                .filter(|marker| marker.kind() == &MarkerKind::Index)
                .map(Marker::position)
                .collect();
            let entry = CaptureRunEntry {
                id,
                ordinal: run.ordinal(),
                source,
                transitions: run.transitions().len() as u64,
                markers: run.markers().len() as u64,
                indices: indices.len() as u64,
                extent: run.transitions().last().copied().unwrap_or(0),
                before_first_index: match indices.first() {
                    Some(first) => count_below(run.transitions(), *first),
                    None => run.transitions().len() as u64,
                },
                after_last_index: match indices.last() {
                    Some(last) => {
                        run.transitions().len() as u64 - count_below(run.transitions(), *last)
                    }
                    None => 0,
                },
                transition_chunks: transitions.len() as u64,
                marker_chunks: markers.len() as u64,
                provenance: run.provenance().clone(),
            };
            self.writer.append(
                SectionKey::new(
                    key.clone(),
                    ScopeId::Run(id),
                    SectionKind::CaptureRunMetadata,
                    0,
                ),
                encode_run_metadata(&entry),
            )?;
            for chunk in &transitions {
                self.writer.append(
                    SectionKey::new(
                        key.clone(),
                        ScopeId::Run(id),
                        SectionKind::TransitionChunk,
                        chunk.ordinal(),
                    ),
                    chunk.payload().to_vec(),
                )?;
            }
            for chunk in &markers {
                self.writer.append(
                    SectionKey::new(
                        key.clone(),
                        ScopeId::Run(id),
                        SectionKind::MarkerChunk,
                        chunk.ordinal(),
                    ),
                    chunk.payload().to_vec(),
                )?;
            }
            track.runs.push(entry);
        }
        self.capture.insert_track(track);

        for run in runs {
            for observation in run.observations(namespace)? {
                let id = self.capture.admit_observation(&key, &observation)?;
                let entry = self
                    .capture
                    .track(&key)
                    .and_then(|track| track.observations().last())
                    .expect("the observation was just admitted")
                    .clone();
                self.writer.append(
                    SectionKey::new(
                        key.clone(),
                        ScopeId::Observation(id),
                        SectionKind::ObservationMetadata,
                        0,
                    ),
                    encode_observation_metadata(&entry),
                )?;
            }
        }
        Ok(())
    }

    /// Closes the backing and hands back the capture it addresses,
    /// together with the sink and its length so the caller can attach
    /// whatever it wrote into.
    pub(crate) fn seal(self) -> Result<(FluxCapture, S, u64)> {
        let (sink, total) = self.writer.seal()?;
        Ok((self.capture, sink, total))
    }
}

/// How many of `transitions` sit strictly below `tick`. The list
/// ascends, so this is a bound rather than a scan.
fn count_below(transitions: &[Tick], tick: Tick) -> u64 {
    transitions.partition_point(|position| *position < tick) as u64
}

impl FluxCapture {
    /// Attaches the backing this capture's sections were written into,
    /// with the working set they are served through.
    pub(crate) fn attach_backing(
        &mut self,
        bytes: Box<dyn ByteSource + Send + Sync>,
        total_bytes: u64,
        cache_bytes: u64,
        known: Vec<&'static str>,
    ) {
        self.backing = Some(CaptureBacking {
            bytes,
            total_bytes,
            cache: Mutex::new(SectionCache::with_bytes(cache_bytes)),
            known,
        });
    }

    /// The backing's total length in bytes.
    pub(crate) fn backing_bytes(&self) -> u64 {
        self.backing
            .as_ref()
            .map_or(0, |backing| backing.total_bytes)
    }

    /// How much of the backing is currently resident.
    pub(crate) fn resident_bytes(&self) -> u64 {
        self.backing.as_ref().map_or(0, |backing| {
            backing
                .cache
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .resident_bytes()
        })
    }
}

/// The version every metadata section this layer writes carries. An
/// unknown one refuses that section rather than being read as this one.
const METADATA_VERSION: u64 = 1;

pub(crate) fn encode_provenance(out: &mut Vec<u8>, provenance: &Provenance) {
    write_text(out, provenance.source);
    write_varint(out, provenance.notes.len() as u64);
    for note in &provenance.notes {
        write_text(out, note);
    }
}

pub(crate) fn decode_provenance(
    source: &str,
    bytes: &[u8],
    at: usize,
    known: &[&'static str],
) -> Result<(Provenance, usize)> {
    let mut cursor = at;
    let (spelling, used) = read_text(source, bytes, cursor)?;
    cursor += used;
    let namespace = known
        .iter()
        .find(|candidate| **candidate == spelling)
        .copied()
        .ok_or_else(|| {
            Error::invalid_image(
                source,
                format!(
                    "backing states the provenance namespace {spelling:?}, which this \
                     reader cannot place, so the backing is not the one this layer wrote"
                ),
            )
        })?;
    let (count, used) = read_varint(source, bytes, cursor)?;
    cursor += used;
    let mut provenance = Provenance::new(namespace);
    for _ in 0..count {
        let (note, used) = read_text(source, bytes, cursor)?;
        cursor += used;
        provenance = provenance.note(note);
    }
    Ok((provenance, cursor - at))
}

fn encode_track_metadata(runs: u64, issues: &[Issue]) -> Vec<u8> {
    let mut out = Vec::new();
    write_varint(&mut out, METADATA_VERSION);
    write_varint(&mut out, runs);
    write_varint(&mut out, issues.len() as u64);
    out
}

fn encode_issues(issues: &[Issue]) -> Vec<u8> {
    let mut out = Vec::new();
    write_varint(&mut out, METADATA_VERSION);
    write_varint(&mut out, issues.len() as u64);
    for issue in issues {
        write_text(&mut out, issue.code);
        write_text(&mut out, &issue.detail);
    }
    out
}

fn encode_run_metadata(entry: &CaptureRunEntry) -> Vec<u8> {
    let mut out = Vec::new();
    write_varint(&mut out, METADATA_VERSION);
    write_varint(&mut out, entry.id.0);
    write_varint(&mut out, entry.ordinal);
    write_varint(&mut out, entry.source.0);
    write_varint(&mut out, entry.transitions);
    write_varint(&mut out, entry.markers);
    write_varint(&mut out, entry.indices);
    write_varint(&mut out, entry.extent);
    write_varint(&mut out, entry.before_first_index);
    write_varint(&mut out, entry.after_last_index);
    write_varint(&mut out, entry.transition_chunks);
    write_varint(&mut out, entry.marker_chunks);
    encode_provenance(&mut out, &entry.provenance);
    out
}

/// Reads one run's metadata section back into the entry it was written
/// from. `known` places the namespaces the reader can resolve.
fn decode_run_metadata(
    source: &str,
    bytes: &[u8],
    known: &[&'static str],
) -> Result<CaptureRunEntry> {
    let mut at = 0;
    let next = |at: &mut usize| -> Result<u64> {
        let (value, used) = read_varint(source, bytes, *at)?;
        *at += used;
        Ok(value)
    };
    let version = next(&mut at)?;
    if version != METADATA_VERSION {
        return Err(Error::invalid_image(
            source,
            format!(
                "backing states run metadata version {version}, which this build has no reading of"
            ),
        ));
    }
    let entry = CaptureRunEntry {
        id: CaptureRunId(next(&mut at)?),
        ordinal: next(&mut at)?,
        source: SourceId(next(&mut at)?),
        transitions: next(&mut at)?,
        markers: next(&mut at)?,
        indices: next(&mut at)?,
        extent: next(&mut at)?,
        before_first_index: next(&mut at)?,
        after_last_index: next(&mut at)?,
        transition_chunks: next(&mut at)?,
        marker_chunks: next(&mut at)?,
        provenance: Provenance::new("flux-capture"),
    };
    let (provenance, _) = decode_provenance(source, bytes, at, known)?;
    Ok(CaptureRunEntry {
        provenance,
        ..entry
    })
}

fn encode_observation_metadata(entry: &ObservationEntry) -> Vec<u8> {
    let mut out = Vec::new();
    write_varint(&mut out, METADATA_VERSION);
    write_varint(&mut out, entry.id.0);
    write_varint(&mut out, entry.ordinal);
    write_varint(&mut out, entry.span);
    write_varint(&mut out, entry.source.run_ordinal());
    write_varint(&mut out, entry.source.start());
    write_varint(&mut out, entry.source.end());
    write_varint(&mut out, entry.transitions);
    write_varint(&mut out, entry.markers);
    encode_provenance(&mut out, &entry.provenance);
    out
}

fn decode_observation_metadata(
    source: &str,
    bytes: &[u8],
    known: &[&'static str],
) -> Result<ObservationEntry> {
    let mut at = 0;
    let next = |at: &mut usize| -> Result<u64> {
        let (value, used) = read_varint(source, bytes, *at)?;
        *at += used;
        Ok(value)
    };
    let version = next(&mut at)?;
    if version != METADATA_VERSION {
        return Err(Error::invalid_image(
            source,
            format!(
                "backing states observation metadata version {version}, which this \
                 build has no reading of"
            ),
        ));
    }
    let id = ObservationId(next(&mut at)?);
    let ordinal = next(&mut at)?;
    let span = next(&mut at)?;
    let slice = CaptureRunSlice::new(next(&mut at)?, next(&mut at)?, next(&mut at)?);
    let transitions = next(&mut at)?;
    let markers = next(&mut at)?;
    let (provenance, _) = decode_provenance(source, bytes, at, known)?;
    Ok(ObservationEntry {
        id,
        ordinal,
        span,
        source: slice,
        transitions,
        markers,
        provenance,
    })
}

#[cfg(test)]
mod tests {
    use super::records::{DerivedCandidate, DerivedKind, SourcePosition};
    use super::*;
    use crate::error::ErrorCategory;

    fn index_at(position: Tick) -> Marker {
        Marker::new(
            position,
            MarkerKind::Index,
            Provenance::new("kryoflux").note("index OOB record"),
        )
    }

    /// A synthetic observation for this layer's own invariant tests,
    /// bounded from the whole of run zero — a coherent slice rather
    /// than a placeholder, since no observation exists without one.
    fn observed(
        source: &'static str,
        span: Tick,
        transitions: Vec<Tick>,
        markers: Vec<Marker>,
    ) -> Result<Observation> {
        Observation::new(
            source,
            Provenance::new(source),
            CaptureRunSlice::new(0, 0, span),
            span,
            transitions,
            markers,
        )
    }

    /// A run as its adapter transferred it, for tests that are about
    /// the run's own invariants rather than about provenance.
    fn captured(
        source: &'static str,
        ordinal: u64,
        transitions: Vec<Tick>,
        markers: Vec<Marker>,
    ) -> Result<CaptureRun> {
        CaptureRun::new(
            source,
            ordinal,
            Provenance::new(source),
            transitions,
            markers,
        )
    }

    fn kryoflux_timebase() -> TimeBase {
        TimeBase::new("kryoflux", 18_432_000 * 73, 56).expect("the rate is stated")
    }

    #[test]
    fn a_capture_holds_a_track_under_the_key_its_adapter_declared() {
        // The adapter names the location in its own terms; the layer
        // stores it and hands it back, having interpreted nothing.
        let key = TrackKey::new("kryoflux", 36, 0);
        let mut capture = FluxCapture::new(kryoflux_timebase());

        capture.insert_track(Track::new(key.clone()));

        assert_eq!(capture.track(&key).map(Track::key), Some(&key));
    }

    #[test]
    fn an_unsupplied_location_differs_from_one_declared_and_holding_nothing() {
        // Two different facts, and the layer is sparse so that they stay
        // different: an absent key means the capture supplied nothing for
        // that location, while a present track with no observations means
        // the source declared the location and had no usable capture of
        // it. Collapsing them would invent evidence in one direction and
        // erase it in the other.
        let declared = TrackKey::new("kryoflux", 36, 0);
        let unsupplied = TrackKey::new("kryoflux", 37, 0);
        let mut capture = FluxCapture::new(kryoflux_timebase());

        capture.insert_track(Track::new(declared.clone()));

        assert!(capture.track(&unsupplied).is_none());
        let track = capture.track(&declared).expect("the location was declared");
        assert!(track.observations().is_empty());
    }

    #[test]
    fn a_fractional_step_and_an_unnumbered_head_survive_unrounded() {
        // The layer's loudest refusal: a TrackKey is not CHS. A source
        // addressing half-tracks, and one numbering no head at all, are
        // carried as stated — 18.5 is neither 18 nor 19, and an absent
        // head is absent rather than quietly becoming head 0.
        let half = SourcePosition::fraction("c1541", 37, 2).expect("the step is stated");
        let key = TrackKey::at("c1541", half, None);

        assert_eq!(key.position(), half);
        assert_eq!(key.head(), None);
        assert_ne!(key.position(), SourcePosition::whole(18));
        assert_ne!(key.position(), SourcePosition::whole(19));
    }

    #[test]
    fn an_observation_names_the_run_and_the_exact_range_it_was_bounded_from() {
        // An observation rebases its transitions to its own origin, so
        // once bounded it can no longer say where in the run it sat.
        // The slice is the whole of the route back, and it keeps the
        // run's own ticks rather than the rebased ones.
        let run = captured(
            "kryoflux",
            3,
            vec![50, 150, 400, 950],
            vec![index_at(100), index_at(900)],
        )
        .unwrap();

        let observations = run.observations("kryoflux").unwrap();
        let slice = observations[0].source();

        assert_eq!(slice.run_ordinal(), 3);
        assert_eq!((slice.start(), slice.end()), (100, 900));
        // Rebased payload, unrebased slice: 150 became 50, and the
        // slice still says the window opened at 100.
        assert_eq!(observations[0].transitions(), [50, 300]);
    }

    #[test]
    fn several_observations_on_one_track_keep_the_source_record_order() {
        // Revolutions arrive in the order the source recorded them, and
        // the ordinal is what preserves that once they are addressed
        // individually. Nothing reorders them by span, transition
        // count, or any other notion of a better pass.
        let run = captured(
            "kryoflux",
            0,
            vec![150, 250, 1050],
            vec![index_at(100), index_at(900), index_at(1700)],
        )
        .unwrap();
        let key = TrackKey::new("kryoflux", 36, 0);
        let mut capture = FluxCapture::new(kryoflux_timebase());
        capture.insert_track(Track::new(key.clone()));
        for observation in run.observations("kryoflux").unwrap() {
            capture
                .admit_observation(&key, &observation)
                .expect("the location was declared");
        }

        let track = capture.track(&key).expect("the location was declared");
        let ordinals: Vec<u64> = track
            .observations()
            .iter()
            .map(ObservationEntry::ordinal)
            .collect();
        assert_eq!(ordinals, [0, 1]);

        // And the ordinals agree with the run they were bounded from,
        // which is the independent record of that order.
        let starts: Vec<Tick> = track
            .observations()
            .iter()
            .map(|observation| observation.source().start())
            .collect();
        assert_eq!(starts, [100, 900]);
    }

    #[test]
    fn a_source_record_the_layer_cannot_name_is_retained_whole_and_in_order() {
        // The two-outcome rule: a source fact either maps to a named
        // field or is kept verbatim as foreign. Nothing is dropped for
        // being unrecognized, because a convenient discard is exactly
        // how a format's blind spot becomes permanent.
        let mut envelope = CaptureEnvelope::new();
        let stream = envelope.declare_source(SourceDescriptor::new(
            "kryoflux",
            "Pinball(1of2)00.0.raw",
            SourceRange::new(0, 184_534),
        ));

        envelope.retain_foreign(ForeignRecord::new(
            stream,
            "kryoflux",
            "oob",
            SourceRange::new(1024, 12),
            vec![1, 2, 3],
        ));
        envelope.retain_foreign(ForeignRecord::new(
            stream,
            "kryoflux",
            "oob",
            SourceRange::new(2048, 4),
            vec![9],
        ));

        let held = envelope.foreign_records();
        assert_eq!(held.len(), 2);
        assert_eq!(held[0].namespace(), "kryoflux");
        assert_eq!(held[0].type_id(), "oob");
        assert_eq!(held[0].payload(), [1, 2, 3]);
        assert_eq!(held[0].range(), SourceRange::new(1024, 12));
        // Retention order is the source's order, and the ordinal is
        // what preserves it once records are addressed individually.
        assert_eq!(
            held.iter().map(ForeignRecord::ordinal).collect::<Vec<_>>(),
            [0, 1]
        );
        assert_eq!(held[1].payload(), [9]);
    }

    #[test]
    fn metadata_keeps_the_sources_own_spelling_and_order_including_repeats() {
        // Not a map. A stream states its keys more than once — two
        // KFInfo blocks, each with its own `host_date` — and both are
        // evidence, so a keyed collection would silently keep one.
        //
        // The values are why the spelling is kept too: a KryoFlux
        // stream declares `sck` as a truncation of 168192000/7, and
        // `ick` as a rounding that is not even that value over eight.
        // The layer records what was written and believes neither.
        let mut envelope = CaptureEnvelope::new();
        let source = envelope.declare_source(SourceDescriptor::new(
            "kryoflux",
            "capture00.0.raw",
            SourceRange::new(0, 184_534),
        ));
        for (key, value) in [
            ("host_date", "2014.11.01"),
            ("sck", "24027428.5714285"),
            ("ick", "3003428.5714285625"),
            ("host_date", "2014.11.02"),
        ] {
            envelope.record_metadata(MetadataRecord::new("kryoflux", source, key, value));
        }

        let held = envelope.metadata();
        // Every stated fact names the member that stated it: with a
        // hundred and sixty-eight of them saying the same keys, a fact
        // whose speaker went unrecorded is one nobody can go back to.
        assert!(held.iter().all(|record| record.source() == source));
        assert_eq!(
            held.iter().map(MetadataRecord::key).collect::<Vec<_>>(),
            ["host_date", "sck", "ick", "host_date"]
        );
        assert_eq!(held[0].value(), "2014.11.01");
        assert_eq!(held[3].value(), "2014.11.02");
        assert_eq!(held[1].value(), "24027428.5714285");
        assert_eq!(
            held.iter().map(MetadataRecord::ordinal).collect::<Vec<_>>(),
            [0, 1, 2, 3]
        );
    }

    #[test]
    fn a_derived_candidate_sits_beside_the_raw_runs_and_never_stands_in_for_them() {
        // A source's own solved stream is its derivative, not proof
        // that the raw capture had one ideal rotation behind it. It is
        // kept, labelled as derived, and reachable only by asking the
        // envelope for it — whatever reads a location's observations
        // never meets it, so it cannot be mistaken for evidence.
        let key = TrackKey::new("a2r", 36, 0);
        let mut capture = FluxCapture::new(kryoflux_timebase());
        capture.insert_track(Track::new(key.clone()));
        capture
            .admit_observation(&key, &observed("a2r", 800, vec![10], Vec::new()).unwrap())
            .expect("the location was declared");

        capture.envelope_mut().retain_derived(DerivedCandidate::new(
            "a2r",
            DerivedKind::SolvedFlux,
            SourceRange::new(4096, 256),
            Provenance::new("a2r").note("source supplied a solved stream"),
        ));

        // The raw evidence is exactly as it was.
        assert_eq!(capture.track(&key).unwrap().observations().len(), 1);
        // And the derivative is held apart, saying what it is.
        let derived = capture.envelope().derived_candidates();
        assert_eq!(derived.len(), 1);
        assert_eq!(derived[0].kind(), &DerivedKind::SolvedFlux);
        assert_eq!(derived[0].range(), SourceRange::new(4096, 256));
    }

    #[test]
    fn a_capture_assembled_from_many_artifacts_keeps_each_source_distinct() {
        // One logical capture is made of many files — the prepared
        // fixture is 168 archive members — so "which artifact did this
        // come from" has to survive assembly. A retained record names
        // the member it was read from, not the set, because a set-wide
        // answer would be no answer at all.
        let mut envelope = CaptureEnvelope::new();
        let first = envelope.declare_source(SourceDescriptor::new(
            "kryoflux",
            "Pinball(1of2)00.0.raw",
            SourceRange::new(0, 184_534),
        ));
        let second = envelope.declare_source(SourceDescriptor::new(
            "kryoflux",
            "Pinball(1of2)00.1.raw",
            SourceRange::new(0, 264_965),
        ));

        envelope.retain_foreign(ForeignRecord::new(
            second,
            "kryoflux",
            "oob",
            SourceRange::new(1024, 12),
            vec![1, 2, 3],
        ));

        assert_ne!(first, second);
        assert_eq!(envelope.sources().len(), 2);
        assert_eq!(
            envelope.source(second).map(SourceDescriptor::artifact),
            Some("Pinball(1of2)00.1.raw")
        );
        // The record traces to one member, and it is the right one.
        assert_eq!(envelope.foreign_records()[0].source(), second);
    }

    #[test]
    fn bounding_keeps_each_markers_own_provenance_beside_the_cut() {
        // Two different facts about one observation. The observation
        // records that it was cut from a run; a marker inside it
        // records how that marker came to be known. Bounding rebuilds
        // its markers from parts, so the second is exactly the fact a
        // careless rebuild drops — leaving a source's index record
        // reading as though this layer had inferred it.
        let index_record = || {
            Marker::new(
                0,
                MarkerKind::Index,
                Provenance::new("kryoflux").note("index OOB record"),
            )
        };
        let run = CaptureRun::new(
            "kryoflux",
            0,
            Provenance::new("kryoflux"),
            vec![150, 400],
            vec![
                Marker::new(100, MarkerKind::Index, index_record().provenance().clone()),
                Marker::new(900, MarkerKind::Index, index_record().provenance().clone()),
            ],
        )
        .unwrap();

        let observations = run.observations("kryoflux").unwrap();
        let observation = &observations[0];

        assert_eq!(
            observation.markers()[0].provenance().notes,
            ["index OOB record"]
        );
        assert!(
            observation.provenance().notes[0].contains("bounded"),
            "{:?}",
            observation.provenance().notes
        );
    }

    #[test]
    fn a_bounded_observation_records_that_it_was_cut_rather_than_recorded() {
        // P4: a fact travels with how it came to be known. These are
        // not revolutions the source handed over — they were cut from a
        // run at its index markers, and the provenance says so, so that
        // nothing downstream can read a derived bound as a recorded
        // rotation.
        let run = CaptureRun::new(
            "kryoflux",
            0,
            Provenance::new("kryoflux").note("stream transferred whole"),
            vec![150, 250, 1050],
            vec![index_at(100), index_at(900), index_at(1700)],
        )
        .unwrap();

        let observations = run.observations("kryoflux").unwrap();

        assert_eq!(run.provenance().notes, ["stream transferred whole"]);
        let bounded = observations[0].provenance();
        assert_eq!(bounded.source, "kryoflux");
        assert!(
            bounded.notes.iter().any(|note| note.contains("index")),
            "the bounding should name its mechanism: {:?}",
            bounded.notes
        );
    }

    #[test]
    fn identity_is_capture_wide_where_the_ordinal_is_only_per_location() {
        // Both locations hold their first observation, so both ordinals
        // are 0. The backing addresses sections by id, so the two must
        // still be tellable apart — that is the whole of what the id is
        // for, and it ranks nothing.
        let first = TrackKey::new("kryoflux", 36, 0);
        let second = TrackKey::new("kryoflux", 38, 0);
        let mut capture = FluxCapture::new(kryoflux_timebase());
        capture.insert_track(Track::new(first.clone()));
        capture.insert_track(Track::new(second.clone()));

        let one = capture
            .admit_observation(
                &first,
                &observed("kryoflux", 800, vec![10], Vec::new()).unwrap(),
            )
            .expect("the location was declared");
        let other = capture
            .admit_observation(
                &second,
                &observed("kryoflux", 800, vec![10], Vec::new()).unwrap(),
            )
            .expect("the location was declared");

        assert_ne!(one, other);
        for key in [&first, &second] {
            let held = &capture.track(key).unwrap().observations()[0];
            assert_eq!(held.ordinal(), 0);
        }
        assert_eq!(capture.track(&first).unwrap().observations()[0].id(), one);
    }

    #[test]
    fn admitting_an_observation_to_an_undeclared_location_is_refused() {
        // Creating the location on the way past would erase the very
        // distinction the sparse layer keeps: a location the capture
        // never supplied would become one it declared.
        let mut capture = FluxCapture::new(kryoflux_timebase());

        let error = capture
            .admit_observation(
                &TrackKey::new("kryoflux", 36, 0),
                &observed("kryoflux", 800, vec![10], Vec::new()).unwrap(),
            )
            .unwrap_err();

        assert_eq!(error.category(), ErrorCategory::InvalidImage);
    }

    #[test]
    fn locations_iterate_in_position_order_whatever_order_they_arrived_in() {
        // 18.5 sorts between 18 and 19. Comparing the reduced parts
        // field by field would put 37/2 last, having compared 37
        // against 19 and stopped there.
        let half = SourcePosition::fraction("c1541", 37, 2).expect("the step is stated");
        let mut capture = FluxCapture::new(kryoflux_timebase());
        for position in [SourcePosition::whole(19), SourcePosition::whole(18), half] {
            capture.insert_track(Track::new(TrackKey::at("c1541", position, None)));
        }

        let order: Vec<SourcePosition> = capture
            .tracks()
            .map(|track| track.key().position())
            .collect();

        assert_eq!(
            order,
            [SourcePosition::whole(18), half, SourcePosition::whole(19)]
        );
    }

    #[test]
    fn a_run_keeps_the_flux_recorded_before_and_after_its_indices() {
        // A real KryoFlux stream brackets its indexed revolutions: the
        // fixture's first track carries 17,168 transitions before the
        // first index. None of that is dropped by bounding.
        let run = captured(
            "kryoflux",
            0,
            vec![50, 150, 400, 950],
            vec![index_at(100), index_at(900)],
        )
        .unwrap();

        assert_eq!(run.transitions(), [50, 150, 400, 950]);
    }

    #[test]
    fn consecutive_indices_bound_one_observation_rebased_to_its_origin() {
        let run = captured(
            "kryoflux",
            0,
            vec![50, 150, 400, 950],
            vec![index_at(100), index_at(900)],
        )
        .unwrap();

        let observations = run.observations("kryoflux").unwrap();

        assert_eq!(observations.len(), 1);
        assert_eq!(observations[0].span(), 800);
        // 150 and 400 fall inside; 50 precedes the first index and 950
        // follows the last, so neither enters the circle.
        assert_eq!(observations[0].transitions(), [50, 300]);
    }

    #[test]
    fn three_indices_bound_two_distinct_observations() {
        let run = captured(
            "kryoflux",
            0,
            vec![150, 250, 1050],
            vec![index_at(100), index_at(900), index_at(1700)],
        )
        .unwrap();

        let observations = run.observations("kryoflux").unwrap();

        assert_eq!(observations.len(), 2);
        assert_eq!(observations[0].transitions(), [50, 150]);
        assert_eq!(observations[1].transitions(), [150]);
    }

    #[test]
    fn a_run_without_two_indices_supplies_no_circular_observation() {
        // Still inspectable capture evidence — it simply cannot state a
        // circumference, and none is invented for it.
        let run = captured("kryoflux", 0, vec![50, 150], vec![index_at(100)]).unwrap();

        assert_eq!(run.transitions(), [50, 150]);
        assert!(run.observations("kryoflux").unwrap().is_empty());
    }

    #[test]
    fn a_run_records_its_transitions_in_recorded_time_order() {
        let error = captured("kryoflux", 0, vec![150, 50], Vec::new()).unwrap_err();

        assert_eq!(error.category(), ErrorCategory::InvalidImage);
    }

    #[test]
    fn a_marker_may_share_a_position_with_a_transition() {
        // Marker channels are parallel timed evidence, so an index
        // pulse coinciding with a reversal is ordinary, not a clash.
        let observation = observed("kryoflux", 800, vec![10, 400], vec![index_at(400)]).unwrap();

        assert_eq!(observation.transitions(), [10, 400]);
        assert_eq!(observation.markers().len(), 1);
        assert_eq!(observation.markers()[0].position(), 400);
    }

    #[test]
    fn markers_keep_their_recorded_order_and_are_never_sorted() {
        // Two markers may share a position, and a later marker may sit
        // earlier on the circle: the source's order is the evidence.
        let markers = vec![
            index_at(700),
            Marker::new(700, MarkerKind::WriteSplice, Provenance::new("a2r")),
            index_at(100),
        ];
        let observation = observed("a2r", 800, vec![10], markers).unwrap();

        let positions: Vec<Tick> = observation.markers().iter().map(Marker::position).collect();
        assert_eq!(positions, [700, 700, 100]);
        assert_eq!(observation.markers()[1].kind(), &MarkerKind::WriteSplice);
    }

    #[test]
    fn a_marker_outside_the_span_is_refused() {
        let error = observed("a2r", 800, vec![10], vec![index_at(800)]).unwrap_err();

        assert_eq!(error.category(), ErrorCategory::InvalidImage);
        assert!(error.to_string().contains("marker"), "{error}");
    }

    #[test]
    fn an_unmodelled_source_event_keeps_its_namespace_code_and_payload() {
        // The two-outcome rule at the marker channel: a source event
        // the layer does not model is retained, never dropped.
        let marker = Marker::new(
            42,
            MarkerKind::SourceMarker {
                namespace: "kryoflux".into(),
                code: 0x0b,
            },
            Provenance::new("kryoflux"),
        )
        .with_payload(vec![1, 2, 3]);
        let observation = observed("kryoflux", 800, Vec::new(), vec![marker]).unwrap();

        assert_eq!(observation.markers()[0].payload(), [1, 2, 3]);
        match observation.markers()[0].kind() {
            MarkerKind::SourceMarker { namespace, code } => {
                assert_eq!(namespace, "kryoflux");
                assert_eq!(*code, 0x0b);
            }
            other => panic!("expected a retained source marker, got {other:?}"),
        }
    }

    #[test]
    fn a_zero_span_is_refused_rather_than_given_an_invented_period() {
        let error = observed("scp", 0, Vec::new(), Vec::new()).unwrap_err();

        assert_eq!(error.category(), ErrorCategory::InvalidImage);
        assert!(error.to_string().contains("span"), "{error}");
    }

    #[test]
    fn an_observation_with_no_transitions_is_valid_evidence() {
        // An unrecorded or blank revolution is something the capture
        // observed, not a malformed record.
        let observation = observed("scp", 800, Vec::new(), Vec::new()).unwrap();

        assert_eq!(observation.span(), 800);
        assert!(observation.transitions().is_empty());
        assert_eq!(observation.wrap_interval(), None);
    }

    #[test]
    fn the_cyclic_wrap_is_implied_by_the_span_not_a_duplicate_pulse() {
        // Last transition at 790, first at 10, circumference 800: the
        // interval closing the circle is 20, and no boundary pulse is
        // stored to say so.
        let observation = observed("scp", 800, vec![10, 400, 790], Vec::new()).unwrap();

        assert_eq!(observation.transitions(), [10, 400, 790]);
        assert_eq!(observation.wrap_interval(), Some(20));
    }

    #[test]
    fn a_lone_transition_wraps_to_itself_across_the_whole_span() {
        let observation = observed("scp", 800, vec![10], Vec::new()).unwrap();

        assert_eq!(observation.wrap_interval(), Some(800));
    }

    #[test]
    fn out_of_order_transitions_are_refused_rather_than_sorted() {
        let error = observed("scp", 800, vec![10, 30, 20], Vec::new()).unwrap_err();

        assert_eq!(error.category(), ErrorCategory::InvalidImage);
        let message = error.to_string();
        assert!(
            message.contains("20"),
            "should name the offending tick: {message}"
        );
        assert!(
            message.contains("30"),
            "should name what it followed: {message}"
        );
    }

    #[test]
    fn a_repeated_transition_position_is_refused() {
        // Strictly increasing, so a duplicate pulse is contradictory
        // evidence rather than something to deduplicate.
        let error = observed("scp", 800, vec![10, 20, 20], Vec::new()).unwrap_err();

        assert_eq!(error.category(), ErrorCategory::InvalidImage);
    }

    #[test]
    fn a_transition_at_or_past_the_span_is_refused_rather_than_wrapped() {
        let error = observed("kryoflux", 800, vec![10, 20, 800], Vec::new()).unwrap_err();

        assert_eq!(error.category(), ErrorCategory::InvalidImage);
        let message = error.to_string();
        assert!(
            message.contains("800"),
            "should name the offending tick: {message}"
        );
        assert!(message.contains("span"), "should name the span: {message}");
    }

    /// A capture of two locations, each one transfer of five
    /// transitions bracketed by three index records, built through the
    /// same route an adapter takes.
    fn built() -> (FluxCapture, TrackKey, TrackKey) {
        let front = TrackKey::new("kryoflux", 0, 0);
        let back = TrackKey::new("kryoflux", 0, 1);
        let mut builder = CaptureBuilder::new(kryoflux_timebase(), Vec::new())
            // Two records a chunk, so the evidence below spans several
            // sections and a read has to walk them.
            .with_chunk_records(2);
        let source = builder.envelope_mut().declare_source(SourceDescriptor::new(
            "kryoflux",
            "capture00.0.raw",
            SourceRange::new(0, 4096),
        ));
        for key in [&front, &back] {
            let run = captured(
                "kryoflux",
                0,
                vec![50, 150, 400, 950, 1400],
                vec![index_at(100), index_at(900), index_at(1300)],
            )
            .unwrap();
            builder
                .add_location(
                    key.clone(),
                    source,
                    std::slice::from_ref(&run),
                    vec![Issue::new("kryoflux-example", "a qualification")],
                )
                .expect("locations arrive in key order");
        }
        let (mut capture, bytes, total) = builder.seal().expect("the backing seals");
        capture.attach_backing(Box::new(Bytes(bytes)), total, 1 << 20, vec!["kryoflux"]);
        (capture, front, back)
    }

    struct Bytes(Vec<u8>);

    impl ByteSource for Bytes {
        fn read_at(&self, offset: u64, into: &mut [u8]) -> Result<()> {
            let at = offset as usize;
            into.copy_from_slice(&self.0[at..at + into.len()]);
            Ok(())
        }
    }

    #[test]
    fn a_built_capture_keeps_only_shape_resident_and_reads_evidence_from_the_backing() {
        let (capture, front, _) = built();
        let track = capture.track(&front).expect("the location was added");
        let run = &track.runs()[0];

        // What stays in memory is a count, not a pulse.
        assert_eq!(run.transitions(), 5);
        assert_eq!(run.markers(), 3);
        assert_eq!(run.indices(), 3);
        assert_eq!(run.extent(), 1400);
        assert_eq!(run.before_first_index(), 1);
        assert_eq!(run.after_last_index(), 1);
        assert_eq!(track.issues()[0].code, "kryoflux-example");

        // And the evidence itself comes back exactly as recorded,
        // including the flux outside the indices.
        assert_eq!(
            capture.run_transitions(&front, run).expect("the run reads"),
            [50, 150, 400, 950, 1400]
        );
        let markers = capture.run_markers(&front, run).expect("the markers read");
        assert_eq!(
            markers.iter().map(Marker::position).collect::<Vec<_>>(),
            [100, 900, 1300]
        );
        assert!(
            markers
                .iter()
                .all(|marker| marker.kind() == &MarkerKind::Index)
        );
    }

    #[test]
    fn an_observation_is_read_back_from_the_run_it_shares_chunks_with() {
        // The backing holds no second copy of an observation's pulses:
        // the slice addresses the run's own chunks, and reading the
        // observation makes the same cut again from the same bytes.
        let (capture, front, _) = built();
        let track = capture.track(&front).expect("the location was added");
        let entry = &track.observations()[0];

        assert_eq!(entry.span(), 800);
        assert_eq!(entry.transitions(), 2);
        assert_eq!((entry.source().start(), entry.source().end()), (100, 900));

        let observation = capture.observation(&front, entry).expect("it reads back");
        assert_eq!(observation.transitions(), [50, 300]);
        assert_eq!(observation.span(), 800);
        assert_eq!(observation.id(), entry.id());
        assert_eq!(observation.ordinal(), entry.ordinal());
        assert_eq!(observation.markers()[0].position(), 0);
    }

    #[test]
    fn one_locations_evidence_loads_without_touching_another() {
        // Two locations, one backing: reading the first leaves the
        // second exactly where it was, which is what a section-keyed
        // backing exists to give.
        let (capture, front, back) = built();
        let front_run = &capture.track(&front).expect("added").runs()[0];
        let back_run = &capture.track(&back).expect("added").runs()[0];

        capture
            .run_transitions(&front, front_run)
            .expect("the first location reads");

        let chunk = |key: &TrackKey, run: &CaptureRunEntry| {
            SectionKey::new(
                key.clone(),
                ScopeId::Run(run.id()),
                SectionKind::TransitionChunk,
                0,
            )
        };
        assert!(capture.holds_section(&chunk(&front, front_run)));
        assert!(!capture.holds_section(&chunk(&back, back_run)));
    }

    #[test]
    fn a_declared_bound_evicts_and_re_reads_rather_than_refusing() {
        // The bound narrows the working set; it never refuses service.
        let front = TrackKey::new("kryoflux", 0, 0);
        let mut builder =
            CaptureBuilder::new(kryoflux_timebase(), Vec::new()).with_chunk_records(1);
        let source = builder.envelope_mut().declare_source(SourceDescriptor::new(
            "kryoflux",
            "capture00.0.raw",
            SourceRange::new(0, 4096),
        ));
        let run = captured(
            "kryoflux",
            0,
            vec![50, 150, 400, 950, 1400],
            vec![index_at(100), index_at(1300)],
        )
        .unwrap();
        builder
            .add_location(
                front.clone(),
                source,
                std::slice::from_ref(&run),
                Vec::new(),
            )
            .expect("one location");
        let (mut capture, bytes, total) = builder.seal().expect("the backing seals");
        // One byte of working set: every chunk still loads, none stays.
        capture.attach_backing(Box::new(Bytes(bytes)), total, 1, vec!["kryoflux"]);

        let entry = &capture.track(&front).expect("added").runs()[0];
        assert_eq!(
            capture.run_transitions(&front, entry).expect("reads"),
            [50, 150, 400, 950, 1400]
        );
        assert!(
            capture.resident_bytes() <= 8,
            "{}",
            capture.resident_bytes()
        );
    }

    #[test]
    fn the_backings_metadata_sections_read_back_into_what_wrote_them() {
        // The sections are what makes the backing self-describing, so
        // they are written to be read rather than only to be counted.
        let (capture, front, _) = built();
        let track = capture.track(&front).expect("added");
        let run = &track.runs()[0];
        let entry = &track.observations()[0];

        let section = capture
            .section(&SectionKey::new(
                front.clone(),
                ScopeId::Run(run.id()),
                SectionKind::CaptureRunMetadata,
                0,
            ))
            .expect("the run metadata reads");
        assert_eq!(
            &decode_run_metadata("kryoflux", &section, &["kryoflux"]).expect("it decodes"),
            run
        );

        let section = capture
            .section(&SectionKey::new(
                front.clone(),
                ScopeId::Observation(entry.id()),
                SectionKind::ObservationMetadata,
                0,
            ))
            .expect("the observation metadata reads");
        assert_eq!(
            &decode_observation_metadata("kryoflux", &section, &["kryoflux"]).expect("it decodes"),
            entry
        );
    }

    #[test]
    fn locations_arriving_out_of_key_order_are_refused() {
        // The section index is ordered, so a builder handed a location
        // out of order is refused rather than writing a backing whose
        // index cannot address it.
        let mut builder = CaptureBuilder::new(kryoflux_timebase(), Vec::new());
        let source = builder.envelope_mut().declare_source(SourceDescriptor::new(
            "kryoflux",
            "capture00.0.raw",
            SourceRange::new(0, 4096),
        ));
        let run = captured("kryoflux", 0, vec![50], Vec::new()).unwrap();
        builder
            .add_location(
                TrackKey::new("kryoflux", 1, 0),
                source,
                std::slice::from_ref(&run),
                Vec::new(),
            )
            .expect("the first location");
        let error = builder
            .add_location(
                TrackKey::new("kryoflux", 0, 0),
                source,
                std::slice::from_ref(&run),
                Vec::new(),
            )
            .expect_err("a location behind the last is refused");

        assert_eq!(error.category(), ErrorCategory::InvalidImage);
    }
}
