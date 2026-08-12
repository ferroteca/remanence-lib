// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The section-addressable backing: bounded evidence, addressed by key
//! and read one section at a time (P27).
//!
//! A capture set is routinely a hundred and sixty-eight members, so
//! nothing here holds a capture's pulses resident. Records stream to a
//! [`ByteSink`], an ordered index is written behind them, and a reader
//! walks the index to the one section it was asked for — the leaf path
//! and that section, and nothing else.
//!
//! [`SectionAddress`] is why this is a seam rather than one concrete
//! key: the backing serves both of the flux family's models (P22), and
//! they do not address alike — a capture by the location its *source*
//! named, a medium by the location its *profile* declares. The index
//! refuses a namespace it cannot place and a section whose bytes
//! changed under it, rather than resolving either to something
//! plausible and serving the wrong evidence from then on.

use std::collections::BTreeMap;

use crate::checksum::crc32;
use crate::error::{Error, Result};

use super::records::{ObservationId, SourcePosition, Tick, TrackKey, greatest_common_divisor};
use super::wire::{read_varint, write_varint};

/// A capture-wide handle for one capture run, on the same terms as an
/// [`ObservationId`]: library-owned identity, never a rank.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct CaptureRunId(pub(super) u64);

/// What a section belongs to.
///
/// The ordering matters: a location's own sections come before those of
/// anything nested in it, so a reader walking the index in key order
/// meets a scope's metadata before the chunks it describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ScopeId {
    Track,
    Run(CaptureRunId),
    Observation(ObservationId),
}

/// What one section holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum SectionKind {
    TrackMetadata,
    CaptureRunMetadata,
    ObservationMetadata,
    TransitionChunk,
    MarkerChunk,
    IssueChunk,
}

/// The complete address of one section of the backing.
///
/// Every part is load-bearing: a location, what within it, of what
/// kind, and which chunk. Two sections never share a key, because a
/// collision would leave one unreachable and serve the other in its
/// place — silently, and as if it were the evidence asked for.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct SectionKey {
    pub(super) track: TrackKey,
    scope: ScopeId,
    kind: SectionKind,
    ordinal: u64,
}

impl SectionKey {
    pub(crate) fn new(track: TrackKey, scope: ScopeId, kind: SectionKind, ordinal: u64) -> Self {
        Self {
            track,
            scope,
            kind,
            ordinal,
        }
    }

    pub(crate) fn track(&self) -> &TrackKey {
        &self.track
    }

    pub(crate) fn scope(&self) -> ScopeId {
        self.scope
    }

    pub(crate) fn kind(&self) -> SectionKind {
        self.kind
    }

    pub(crate) fn ordinal(&self) -> u64 {
        self.ordinal
    }
}

/// The exact event range, in a run's own ticks, that one circular
/// observation was bounded from.
///
/// Bounding rebases an observation's events to its own origin, so the
/// observation itself can no longer say where in the run it sat. This
/// is the whole of the route back to the evidence as recorded, which is
/// why an observation cannot exist without one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CaptureRunSlice {
    run_ordinal: u64,
    start: Tick,
    end: Tick,
}

impl CaptureRunSlice {
    pub(crate) fn new(run_ordinal: u64, start: Tick, end: Tick) -> Self {
        Self {
            run_ordinal,
            start,
            end,
        }
    }

    pub(crate) fn run_ordinal(&self) -> u64 {
        self.run_ordinal
    }

    pub(crate) fn start(&self) -> Tick {
        self.start
    }

    pub(crate) fn end(&self) -> Tick {
        self.end
    }
}

/// What a backing may be keyed by.
///
/// The backing is one mechanism serving both of the flux family's
/// models (P22), and they do not address alike: a capture is addressed
/// by the location its *source* named, a medium by the location its
/// *profile* declares. Sharing the record stream and the ordered index
/// while keeping the two addressings apart is the whole of why this is
/// a seam rather than one concrete key.
pub(crate) trait SectionAddress: Clone + Ord {
    /// The namespace a refusal about this section reads under (P4).
    fn namespace(&self) -> &'static str;

    /// Appends this address, self-delimiting so an index node can hold
    /// a run of them without a separate length table.
    fn write(&self, out: &mut Vec<u8>);

    /// Reads one address back, returning it and the bytes it used.
    ///
    /// `known` is the set of namespaces this reader can place. The
    /// backing is private session state, so a namespace outside that
    /// set means the index is not the one this layer wrote, and the
    /// layer refuses rather than resolving it to something plausible
    /// and addressing the wrong sections from then on.
    fn read(source: &str, bytes: &[u8], at: usize, known: &[&'static str])
    -> Result<(Self, usize)>;
}

impl SectionAddress for SectionKey {
    fn namespace(&self) -> &'static str {
        self.track.namespace
    }

    fn write(&self, out: &mut Vec<u8>) {
        write_section_key(out, self);
    }

    fn read(
        source: &str,
        bytes: &[u8],
        at: usize,
        known: &[&'static str],
    ) -> Result<(Self, usize)> {
        read_section_key(source, bytes, at, known)
    }
}

/// Writes one complete section key, self-delimiting so a node can hold
/// a run of them without a separate length table.
fn write_section_key(out: &mut Vec<u8>, key: &SectionKey) {
    let namespace = key.track.namespace.as_bytes();
    write_varint(out, namespace.len() as u64);
    out.extend_from_slice(namespace);
    write_varint(out, key.track.position.numerator);
    write_varint(out, key.track.position.denominator);
    // Zero is no head at all, which is a different fact from head zero.
    match key.track.head {
        None => write_varint(out, 0),
        Some(head) => write_varint(out, head.saturating_add(1)),
    }
    match key.scope {
        ScopeId::Track => write_varint(out, 0),
        ScopeId::Run(CaptureRunId(id)) => {
            write_varint(out, 1);
            write_varint(out, id);
        }
        ScopeId::Observation(ObservationId(id)) => {
            write_varint(out, 2);
            write_varint(out, id);
        }
    }
    write_varint(
        out,
        match key.kind {
            SectionKind::TrackMetadata => 0,
            SectionKind::CaptureRunMetadata => 1,
            SectionKind::ObservationMetadata => 2,
            SectionKind::TransitionChunk => 3,
            SectionKind::MarkerChunk => 4,
            SectionKind::IssueChunk => 5,
        },
    );
    write_varint(out, key.ordinal);
}

/// Reads one complete section key, returning it and the bytes it used.
///
/// `known` is the set of namespaces this reader can place. The backing
/// is private session state, so a namespace outside that set means the
/// index is not the one this layer wrote, and the layer refuses rather
/// than resolving it to something plausible and addressing the wrong
/// sections from then on.
fn read_section_key(
    source: &str,
    bytes: &[u8],
    at: usize,
    known: &[&'static str],
) -> Result<(SectionKey, usize)> {
    let mut cursor = at;
    let (length, used) = read_varint(source, bytes, cursor)?;
    cursor += used;
    let length = usize::try_from(length).map_err(|_| {
        Error::invalid_image(
            source,
            "index names a namespace longer than this host can address",
        )
    })?;
    let raw = bytes
        .get(cursor..cursor + length)
        .ok_or_else(|| Error::invalid_image(source, "index ends inside the namespace it names"))?;
    cursor += length;
    let spelling = std::str::from_utf8(raw)
        .map_err(|_| Error::invalid_image(source, "index names a namespace that is not text"))?;
    let namespace = known
        .iter()
        .find(|candidate| **candidate == spelling)
        .copied()
        .ok_or_else(|| {
            Error::invalid_image(
                source,
                format!(
                    "index names the namespace {spelling:?}, which this reader cannot \
                     place, so the index is not the one this layer wrote"
                ),
            )
        })?;

    let (numerator, used) = read_varint(source, bytes, cursor)?;
    cursor += used;
    let (denominator, used) = read_varint(source, bytes, cursor)?;
    cursor += used;
    if denominator == 0 {
        return Err(Error::invalid_image(
            source,
            "index states a location over zero steps, which is no position",
        ));
    }
    // A written position is always reduced, so an unreduced one is
    // corruption — and reducing it here would collide with the key
    // already indexed under its reduced form, leaving one of the two
    // sections unreachable while the other answered for it.
    if greatest_common_divisor(numerator, denominator) != 1 {
        return Err(Error::invalid_image(
            source,
            format!(
                "index states the location {numerator}/{denominator}, which is not \
                 in the reduced form every written key carries"
            ),
        ));
    }
    let (head, used) = read_varint(source, bytes, cursor)?;
    cursor += used;
    let (scope_tag, used) = read_varint(source, bytes, cursor)?;
    cursor += used;
    let scope = match scope_tag {
        0 => ScopeId::Track,
        1 | 2 => {
            let (id, used) = read_varint(source, bytes, cursor)?;
            cursor += used;
            if scope_tag == 1 {
                ScopeId::Run(CaptureRunId(id))
            } else {
                ScopeId::Observation(ObservationId(id))
            }
        }
        other => {
            return Err(Error::invalid_image(
                source,
                format!(
                    "index states a section scope {other}, which this version has no reading of"
                ),
            ));
        }
    };
    let (kind_tag, used) = read_varint(source, bytes, cursor)?;
    cursor += used;
    let kind = match kind_tag {
        0 => SectionKind::TrackMetadata,
        1 => SectionKind::CaptureRunMetadata,
        2 => SectionKind::ObservationMetadata,
        3 => SectionKind::TransitionChunk,
        4 => SectionKind::MarkerChunk,
        5 => SectionKind::IssueChunk,
        other => {
            return Err(Error::invalid_image(
                source,
                format!(
                    "index states a section kind {other}, which this version has no reading of"
                ),
            ));
        }
    };
    let (ordinal, used) = read_varint(source, bytes, cursor)?;
    cursor += used;

    Ok((
        SectionKey {
            track: TrackKey {
                namespace,
                position: SourcePosition {
                    numerator,
                    denominator,
                },
                head: head.checked_sub(1),
            },
            scope,
            kind,
            ordinal,
        },
        cursor - at,
    ))
}

/// Where one section sits in the backing, and what it should check to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SectionLocation {
    offset: u64,
    length: u64,
    checksum: u32,
}

impl SectionLocation {
    pub(crate) fn offset(&self) -> u64 {
        self.offset
    }

    pub(crate) fn length(&self) -> u64 {
        self.length
    }

    pub(crate) fn checksum(&self) -> u32 {
        self.checksum
    }
}

/// How many sections one index leaf describes.
///
/// Tests build backings with a tiny capacity to force several leaves
/// out of a handful of sections, the way the cache tests use a tiny
/// bound to force eviction.
pub(crate) const LEAF_ENTRIES: usize = 64;

/// Marks the end of a backing, and says which shape it is.
const FOOTER_MAGIC: u32 = 0x464c_5831;
const FOOTER_VERSION: u32 = 1;
/// magic, version, root offset, root length.
const FOOTER_BYTES: usize = 4 + 4 + 8 + 8;

/// Somewhere the backing's bytes can be read from at an offset.
///
/// The seam exists so the layer's own tests can assert what a read
/// touched, and so the backing does not care whether its bytes are in
/// private session storage or anywhere else.
pub(crate) trait ByteSource {
    fn read_at(&self, offset: u64, into: &mut [u8]) -> Result<()>;
}

/// Somewhere a backing's bytes are appended as it is built.
///
/// The seam is the other half of [`ByteSource`], and it exists for the
/// same reason the reader is bounded: a capture assembled from a
/// hundred and sixty-eight members would otherwise be built whole in
/// memory before a byte of it could be read back, which is the one
/// thing P27 says a session never does. A vector sink serves the
/// layer's own tests; private session storage serves an adapter.
pub(crate) trait ByteSink {
    fn append(&mut self, bytes: &[u8]) -> Result<()>;
}

impl ByteSink for Vec<u8> {
    fn append(&mut self, bytes: &[u8]) -> Result<()> {
        self.extend_from_slice(bytes);
        Ok(())
    }
}

/// Private session storage a backing streams into (P27).
///
/// It belongs here rather than beside either model because both stream
/// into the same mechanism: what differs between a capture and a medium
/// is the address, not the storage.
pub(crate) struct SessionBacking {
    file: std::sync::Arc<std::fs::File>,
    written: u64,
}

impl SessionBacking {
    pub(crate) fn create() -> Result<Self> {
        Ok(Self {
            file: std::sync::Arc::new(crate::io::cache::session_storage_file()?),
            written: 0,
        })
    }

    /// The same storage, read back a bounded section at a time.
    pub(crate) fn into_source(self) -> SessionSource {
        SessionSource(self.file)
    }
}

impl ByteSink for SessionBacking {
    fn append(&mut self, bytes: &[u8]) -> Result<()> {
        crate::io::device::write_all_at(&self.file, self.written, bytes)
            .map_err(|error| Error::io(format!("failed to write a layer backing: {error}")))?;
        self.written += bytes.len() as u64;
        Ok(())
    }
}

pub(crate) struct SessionSource(std::sync::Arc<std::fs::File>);

impl ByteSource for SessionSource {
    fn read_at(&self, offset: u64, into: &mut [u8]) -> Result<()> {
        crate::io::device::read_exact_at(&self.0, offset, into)
            .map_err(|error| Error::io(format!("failed to read a layer backing: {error}")))
    }
}

/// Builds a backing by appending sections in key order.
///
/// The index is finished only once every section it references is
/// complete, which is what makes a half-written backing detectable
/// rather than a layer with holes in it.
#[derive(Debug)]
pub(crate) struct SectionWriter<K: SectionAddress, S: ByteSink = Vec<u8>> {
    sink: S,
    /// How many bytes have gone into the sink, which is where the next
    /// section will sit. The sink itself is not asked: it may be a file
    /// this writer is not the only holder of.
    written: u64,
    entries: Vec<(K, SectionLocation)>,
    last: Option<K>,
    leaf_entries: usize,
}

impl<K: SectionAddress> Default for SectionWriter<K, Vec<u8>> {
    fn default() -> Self {
        Self::with_leaf_capacity(LEAF_ENTRIES)
    }
}

impl<K: SectionAddress> SectionWriter<K, Vec<u8>> {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// A writer whose index leaves hold at most `leaf_entries` sections.
    pub(crate) fn with_leaf_capacity(leaf_entries: usize) -> Self {
        Self::to_sink(Vec::new(), leaf_entries)
    }

    /// Closes a backing built in memory. Infallible where the general
    /// form is not: a vector accepts every byte offered to it.
    pub(crate) fn finish(self) -> Vec<u8> {
        self.seal().expect("a vector sink accepts every byte").0
    }
}

impl<K: SectionAddress, S: ByteSink> SectionWriter<K, S> {
    /// A writer emitting into `sink`, whose index leaves hold at most
    /// `leaf_entries` sections.
    pub(crate) fn to_sink(sink: S, leaf_entries: usize) -> Self {
        Self {
            sink,
            written: 0,
            entries: Vec::new(),
            last: None,
            leaf_entries: leaf_entries.max(1),
        }
    }

    /// Appends one section, which must sort after every section already
    /// appended.
    pub(crate) fn append(&mut self, key: K, payload: Vec<u8>) -> Result<()> {
        if let Some(last) = &self.last
            && &key <= last
        {
            return Err(Error::invalid_image(
                key.namespace(),
                "a section does not sort after the one appended before it, so the \
                 backing is not being emitted in key order",
            ));
        }
        let location = SectionLocation {
            offset: self.written,
            length: payload.len() as u64,
            checksum: crc32(&payload),
        };
        self.sink.append(&payload)?;
        self.written += payload.len() as u64;
        self.entries.push((key.clone(), location));
        self.last = Some(key);
        Ok(())
    }

    /// Closes the backing: leaves in key order, then a root naming each
    /// leaf's first key, then the fixed footer that locates the root.
    /// Returns the sink and the backing's total length.
    ///
    /// The index is appended only once every section it references is
    /// complete, so a backing cut short has no footer and is refused
    /// rather than exposed as a layer with holes in it.
    pub(crate) fn seal(mut self) -> Result<(S, u64)> {
        let mut leaves: Vec<(K, u64, u64)> = Vec::new();
        for run in self.entries.chunks(self.leaf_entries) {
            let mut leaf = Vec::new();
            write_varint(&mut leaf, run.len() as u64);
            for (key, location) in run {
                key.write(&mut leaf);
                write_varint(&mut leaf, location.offset);
                write_varint(&mut leaf, location.length);
                write_varint(&mut leaf, u64::from(location.checksum));
            }
            let offset = self.written;
            self.sink.append(&leaf)?;
            self.written += leaf.len() as u64;
            leaves.push((run[0].0.clone(), offset, leaf.len() as u64));
        }

        let mut root = Vec::new();
        write_varint(&mut root, leaves.len() as u64);
        for (first, offset, length) in &leaves {
            first.write(&mut root);
            write_varint(&mut root, *offset);
            write_varint(&mut root, *length);
        }
        let root_offset = self.written;
        self.sink.append(&root)?;
        self.written += root.len() as u64;

        let mut footer = Vec::with_capacity(FOOTER_BYTES);
        footer.extend_from_slice(&FOOTER_MAGIC.to_le_bytes());
        footer.extend_from_slice(&FOOTER_VERSION.to_le_bytes());
        footer.extend_from_slice(&root_offset.to_le_bytes());
        footer.extend_from_slice(&(root.len() as u64).to_le_bytes());
        self.sink.append(&footer)?;
        self.written += footer.len() as u64;
        Ok((self.sink, self.written))
    }
}

/// Finds where one section sits, reading only the index path to it.
///
/// Three bounded reads whatever the capture's size: the fixed footer,
/// the root, and the one leaf whose range covers the key. The index is
/// never resident whole, which is what lets a capture of any size open
/// under a bound that knew nothing of that size.
pub(crate) fn locate_section<K: SectionAddress>(
    source: &dyn ByteSource,
    total_bytes: u64,
    key: &K,
    known: &[&'static str],
) -> Result<Option<SectionLocation>> {
    let namespace = key.namespace();
    if total_bytes < FOOTER_BYTES as u64 {
        return Err(Error::invalid_image(
            namespace,
            "backing is shorter than its own footer, so no index was ever appended",
        ));
    }
    let mut footer = [0u8; FOOTER_BYTES];
    source.read_at(total_bytes - FOOTER_BYTES as u64, &mut footer)?;
    if u32::from_le_bytes([footer[0], footer[1], footer[2], footer[3]]) != FOOTER_MAGIC {
        return Err(Error::invalid_image(
            namespace,
            "backing does not end in an index footer, so it was never completed",
        ));
    }
    let version = u32::from_le_bytes([footer[4], footer[5], footer[6], footer[7]]);
    if version != FOOTER_VERSION {
        return Err(Error::invalid_image(
            namespace,
            format!(
                "backing states index version {version}, which this build has no \
                 reading of"
            ),
        ));
    }
    let root_offset = u64::from_le_bytes(footer[8..16].try_into().expect("eight bytes"));
    let root_length = u64::from_le_bytes(footer[16..24].try_into().expect("eight bytes"));
    let root = read_span(source, namespace, total_bytes, root_offset, root_length)?;

    // Descend: the last leaf whose first key does not exceed the wanted
    // one is the only leaf that can hold it.
    let mut at = 0;
    let (count, used) = read_varint(namespace, &root, at)?;
    at += used;
    let mut chosen: Option<(u64, u64)> = None;
    for _ in 0..count {
        let (first, used) = K::read(namespace, &root, at, known)?;
        at += used;
        let (offset, used) = read_varint(namespace, &root, at)?;
        at += used;
        let (length, used) = read_varint(namespace, &root, at)?;
        at += used;
        if &first <= key {
            chosen = Some((offset, length));
        } else {
            break;
        }
    }
    let Some((offset, length)) = chosen else {
        return Ok(None);
    };

    let leaf = read_span(source, namespace, total_bytes, offset, length)?;
    let mut at = 0;
    let (count, used) = read_varint(namespace, &leaf, at)?;
    at += used;
    for _ in 0..count {
        let (held, used) = K::read(namespace, &leaf, at, known)?;
        at += used;
        let (offset, used) = read_varint(namespace, &leaf, at)?;
        at += used;
        let (length, used) = read_varint(namespace, &leaf, at)?;
        at += used;
        let (checksum, used) = read_varint(namespace, &leaf, at)?;
        at += used;
        if &held == key {
            let checksum = u32::try_from(checksum).map_err(|_| {
                Error::invalid_image(namespace, "index states a check wider than a CRC-32")
            })?;
            return Ok(Some(SectionLocation {
                offset,
                length,
                checksum,
            }));
        }
    }
    Ok(None)
}

/// Reads one bounded span, refusing one the backing cannot contain
/// before anything is sought or allocated.
fn read_span(
    source: &dyn ByteSource,
    namespace: &'static str,
    total_bytes: u64,
    offset: u64,
    length: u64,
) -> Result<Vec<u8>> {
    if offset
        .checked_add(length)
        .is_none_or(|end| end > total_bytes)
    {
        return Err(Error::invalid_image(
            namespace,
            format!(
                "index points at bytes {offset}..+{length}, which lie outside the \
                 backing's {total_bytes} bytes"
            ),
        ));
    }
    let length = usize::try_from(length).map_err(|_| {
        Error::invalid_image(
            namespace,
            "index states a record longer than this host can address",
        )
    })?;
    let mut span = vec![0u8; length];
    source.read_at(offset, &mut span)?;
    Ok(span)
}

/// Reads one section, and nothing else.
///
/// The index says where the record is and what it checks to, both of
/// which are verified before the bytes are believed — a record that
/// changed under the index is refused rather than handed back as flux
/// that never existed.
pub(crate) fn read_section<K: SectionAddress>(
    source: &dyn ByteSource,
    total_bytes: u64,
    key: &K,
    known: &[&'static str],
) -> Result<Vec<u8>> {
    let location = locate_section(source, total_bytes, key, known)?.ok_or_else(|| {
        Error::invalid_image(
            key.namespace(),
            "backing holds no section under the address asked for",
        )
    })?;
    let payload = read_span(
        source,
        key.namespace(),
        total_bytes,
        location.offset,
        location.length,
    )?;
    if crc32(&payload) != location.checksum {
        return Err(Error::invalid_image(
            key.namespace(),
            format!(
                "section at byte {} does not check to what the index states, so \
                 its bytes are not the ones that were written",
                location.offset
            ),
        ));
    }
    Ok(payload)
}

/// This layer's bounded working set of decoded sections (P27).
///
/// Caching is per modeled durable layer under one declared session
/// budget, so this is the capture's own and not the device stack's: that
/// one is addressed by extent offset over a virtual disk, and a
/// capture is addressed by section key.
///
/// Every entry is clean. A capture is read evidence — a modelled write
/// has no coherent destination in it, since nothing says which of
/// several disagreeing observations a drive would overwrite — so there
/// is no dirty class here to spill. Writes land in the medium reduced
/// from the capture, which reuses this backing and brings that half of
/// the policy with it.
#[derive(Debug)]
pub(crate) struct SectionCache<K: SectionAddress> {
    resident: BTreeMap<K, CachedSection>,
    bytes_resident: u64,
    bound: u64,
    clock: u64,
}

#[derive(Debug)]
struct CachedSection {
    payload: Vec<u8>,
    /// When this entry was last served, for choosing what to drop.
    used: u64,
}

impl<K: SectionAddress> SectionCache<K> {
    /// A cache bounded at `bytes` of resident section payloads.
    ///
    /// A bound smaller than one section narrows the working set rather
    /// than refusing service: the section still loads, it simply does
    /// not stay.
    pub(crate) fn with_bytes(bytes: u64) -> Self {
        Self {
            resident: BTreeMap::new(),
            bytes_resident: 0,
            bound: bytes,
            clock: 0,
        }
    }

    pub(crate) fn resident_bytes(&self) -> u64 {
        self.bytes_resident
    }

    /// Whether this section is currently in the working set.
    pub(crate) fn holds(&self, key: &K) -> bool {
        self.resident.contains_key(key)
    }

    /// Serves one section, reading it through the index on a miss.
    ///
    /// A hit costs no I/O. A miss reads one bounded record range and
    /// never a whole capture, which is the promise the whole
    /// section-addressed backing exists to keep.
    pub(crate) fn section(
        &mut self,
        source: &dyn ByteSource,
        total_bytes: u64,
        key: &K,
        known: &[&'static str],
    ) -> Result<&[u8]> {
        self.clock += 1;
        if !self.resident.contains_key(key) {
            let payload = read_section(source, total_bytes, key, known)?;
            self.make_room(payload.len() as u64);
            self.bytes_resident += payload.len() as u64;
            self.resident.insert(
                key.clone(),
                CachedSection {
                    payload,
                    used: self.clock,
                },
            );
        }
        let clock = self.clock;
        let entry = self.resident.get_mut(key).expect("just made resident");
        entry.used = clock;
        Ok(&entry.payload)
    }

    /// Drops least-recently-served entries until `wanted` more bytes
    /// fit. Clean state is always evictable, so this never fails.
    fn make_room(&mut self, wanted: u64) {
        while !self.resident.is_empty() && self.bytes_resident + wanted > self.bound {
            let coldest = self
                .resident
                .iter()
                .min_by_key(|(_, entry)| entry.used)
                .map(|(key, _)| key.clone())
                .expect("the map is not empty");
            if let Some(dropped) = self.resident.remove(&coldest) {
                self.bytes_resident -= dropped.payload.len() as u64;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::wire::{decode_transitions, split_transitions};
    use super::*;
    use crate::error::ErrorCategory;

    #[test]
    fn a_section_key_addresses_one_thing_and_keys_order_deterministically() {
        // Sections are emitted in key order and the index is searched
        // in that order, so it has to be total and stable. Sections of
        // one location are told apart by scope, then kind, then chunk
        // ordinal — a collision would make one section unreachable and
        // silently serve another in its place.
        let track = TrackKey::new("kryoflux", 36, 0);
        let later = TrackKey::new("kryoflux", 38, 0);
        let run = ScopeId::Run(CaptureRunId(0));
        let observation = ScopeId::Observation(ObservationId(0));

        let mut keys = vec![
            SectionKey::new(later.clone(), ScopeId::Track, SectionKind::TrackMetadata, 0),
            SectionKey::new(track.clone(), observation, SectionKind::TransitionChunk, 1),
            SectionKey::new(track.clone(), observation, SectionKind::TransitionChunk, 0),
            SectionKey::new(track.clone(), observation, SectionKind::MarkerChunk, 0),
            SectionKey::new(track.clone(), run, SectionKind::CaptureRunMetadata, 0),
            SectionKey::new(track.clone(), ScopeId::Track, SectionKind::TrackMetadata, 0),
        ];
        let scrambled = keys.clone();
        keys.sort();

        // Location first, so one track's sections are contiguous.
        assert_eq!(keys[0].track(), &track);
        assert_eq!(keys[5].track(), &later);
        // Within a location: track scope, then run, then observation.
        assert_eq!(keys[0].scope(), ScopeId::Track);
        assert_eq!(keys[1].scope(), run);
        // Within one scope, chunk ordinals ascend.
        let chunks: Vec<u64> = keys
            .iter()
            .filter(|key| key.kind() == SectionKind::TransitionChunk)
            .map(SectionKey::ordinal)
            .collect();
        assert_eq!(chunks, [0, 1]);
        // And no two of these six are the same section.
        let mut unique = scrambled;
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), 6);
    }

    /// A byte source that records every range asked of it, so a test
    /// can assert what a read actually touched rather than trusting it.
    struct CountingSource {
        bytes: Vec<u8>,
        reads: std::cell::RefCell<Vec<(u64, u64)>>,
    }

    impl CountingSource {
        fn new(bytes: Vec<u8>) -> Self {
            Self {
                bytes,
                reads: std::cell::RefCell::new(Vec::new()),
            }
        }

        fn reads(&self) -> Vec<(u64, u64)> {
            self.reads.borrow().clone()
        }
    }

    impl ByteSource for CountingSource {
        fn read_at(&self, offset: u64, into: &mut [u8]) -> Result<()> {
            self.reads.borrow_mut().push((offset, into.len() as u64));
            let at = offset as usize;
            into.copy_from_slice(&self.bytes[at..at + into.len()]);
            Ok(())
        }
    }

    /// The namespaces this layer's own tests can place.
    const KNOWN: &[&str] = &["kryoflux", "c1541", "a2r", "scp"];

    fn transition_section(track: &TrackKey, ordinal: u64) -> (SectionKey, Vec<Tick>, Vec<u8>) {
        let ticks: Vec<Tick> = (0..50).map(|n| ordinal * 1000 + n * 3).collect();
        let chunk = split_transitions(&ticks, 50).expect("the ticks ascend");
        let key = SectionKey::new(
            track.clone(),
            ScopeId::Observation(ObservationId(0)),
            SectionKind::TransitionChunk,
            ordinal,
        );
        (key, ticks, chunk[0].payload().to_vec())
    }

    #[test]
    fn loading_one_section_reads_only_the_index_path_and_that_section() {
        // The P27 promise, now that the index is in the backing too: a
        // miss reads one bounded record range plus the index path to
        // it — the fixed footer, the root, and the one leaf that can
        // hold the key. Never a leaf that cannot, and never the whole
        // capture.
        let track = TrackKey::new("kryoflux", 36, 0);
        // Two sections per leaf, so six sections make three leaves.
        let mut writer = SectionWriter::with_leaf_capacity(2);
        let mut wanted = None;
        for ordinal in 0..6 {
            let (key, ticks, payload) = transition_section(&track, ordinal);
            if ordinal == 5 {
                wanted = Some((key.clone(), ticks));
            }
            writer.append(key, payload).expect("keys ascend");
        }
        let bytes = writer.finish();
        let total = bytes.len() as u64;
        let (key, expected) = wanted.unwrap();

        let source = CountingSource::new(bytes);
        let payload = read_section(&source, total, &key, KNOWN).expect("the section is indexed");

        assert_eq!(decode_transitions(&payload).unwrap(), expected);

        // Footer, root, one leaf, one section: four bounded reads, and
        // the count does not grow with the number of leaves.
        let reads = source.reads();
        assert_eq!(reads.len(), 4, "{reads:?}");
        assert_eq!(reads[0].1, FOOTER_BYTES as u64);
        let located = locate_section(&source, total, &key, KNOWN)
            .unwrap()
            .expect("the section is indexed");
        assert_eq!(reads[3], (located.offset(), located.length()));
        // Nothing read spanned the whole backing.
        assert!(reads.iter().all(|(_, length)| *length < total), "{reads:?}");
    }

    #[test]
    fn a_section_touched_twice_is_read_once() {
        // P27's hit. The working set is whatever the operation touched,
        // and touching it again costs nothing: the second request is
        // served from what the first decoded, index path included.
        let track = TrackKey::new("kryoflux", 36, 0);
        let mut writer = SectionWriter::new();
        let (key, ticks, payload) = transition_section(&track, 0);
        writer.append(key.clone(), payload).expect("keys ascend");
        let bytes = writer.finish();
        let total = bytes.len() as u64;

        let source = CountingSource::new(bytes);
        let mut cache = SectionCache::with_bytes(64 * 1024);

        let first = cache
            .section(&source, total, &key, KNOWN)
            .expect("the section is indexed")
            .to_vec();
        let after_first = source.reads().len();
        let second = cache
            .section(&source, total, &key, KNOWN)
            .expect("the section is indexed")
            .to_vec();

        assert_eq!(decode_transitions(&first).unwrap(), ticks);
        assert_eq!(first, second);
        assert_eq!(
            source.reads().len(),
            after_first,
            "the second request re-read"
        );
    }

    #[test]
    fn a_clean_section_evicts_under_the_bound_and_re_reads_from_the_backing() {
        // The other half of P27: clean state is always evictable,
        // sound because the P7 claim pins the source, so a dropped
        // section re-reads at will. A bound too small for two sections
        // still serves both — it narrows the working set, it never
        // refuses.
        let track = TrackKey::new("kryoflux", 36, 0);
        let mut writer = SectionWriter::new();
        let mut keys = Vec::new();
        for ordinal in 0..2 {
            let (key, _, payload) = transition_section(&track, ordinal);
            keys.push(key.clone());
            writer.append(key, payload).expect("keys ascend");
        }
        let bytes = writer.finish();
        let total = bytes.len() as u64;

        let source = CountingSource::new(bytes);
        let one_section = locate_section(&source, total, &keys[0], KNOWN)
            .unwrap()
            .expect("the section is indexed")
            .length();
        let mut cache = SectionCache::with_bytes(one_section);

        cache.section(&source, total, &keys[0], KNOWN).unwrap();
        cache.section(&source, total, &keys[1], KNOWN).unwrap();
        let before_reload = source.reads().len();
        cache.section(&source, total, &keys[0], KNOWN).unwrap();

        // The first was evicted to admit the second, so asking again
        // costs reads: it came back from the backing rather than
        // having been kept.
        assert!(source.reads().len() > before_reload);
        assert!(cache.resident_bytes() <= one_section);
    }

    #[test]
    fn loading_one_section_neither_materializes_nor_invalidates_another() {
        // Sections are addressed individually the whole way down, so
        // work on one is work on one. A backing that pulled in
        // neighbours would turn a bounded read into a cascade; one that
        // invalidated them would make every read cost the last one.
        let track = TrackKey::new("kryoflux", 36, 0);
        let mut writer = SectionWriter::new();
        let mut keys = Vec::new();
        for ordinal in 0..3 {
            let (key, _, payload) = transition_section(&track, ordinal);
            keys.push(key.clone());
            writer.append(key, payload).expect("keys ascend");
        }
        let bytes = writer.finish();
        let total = bytes.len() as u64;

        let source = CountingSource::new(bytes);
        let mut cache = SectionCache::with_bytes(64 * 1024);

        cache.section(&source, total, &keys[0], KNOWN).unwrap();
        cache.section(&source, total, &keys[2], KNOWN).unwrap();

        // The section between them was never touched.
        assert!(!cache.holds(&keys[1]));

        // And the first survived the second: it serves without I/O.
        let settled = source.reads().len();
        cache.section(&source, total, &keys[0], KNOWN).unwrap();
        assert_eq!(source.reads().len(), settled);
    }

    #[test]
    fn a_section_key_round_trips_through_the_encoding_the_index_stores() {
        // Every part of the key is addressing, so every part has to
        // survive: a fractional position, an unnumbered head, and each
        // scope and kind. What comes back has to sort where it sorted
        // before, or the index it was written into no longer describes
        // where anything is.
        let known = ["kryoflux", "c1541"];
        let keys = [
            SectionKey::new(
                TrackKey::new("kryoflux", 36, 1),
                ScopeId::Observation(ObservationId(7)),
                SectionKind::TransitionChunk,
                3,
            ),
            SectionKey::new(
                TrackKey::at(
                    "c1541",
                    SourcePosition::fraction("c1541", 37, 2).unwrap(),
                    None,
                ),
                ScopeId::Track,
                SectionKind::TrackMetadata,
                0,
            ),
            SectionKey::new(
                TrackKey::new("c1541", 0, 0),
                ScopeId::Run(CaptureRunId(0)),
                SectionKind::MarkerChunk,
                u64::MAX,
            ),
        ];

        for key in &keys {
            let mut encoded = Vec::new();
            write_section_key(&mut encoded, key);
            let (decoded, used) =
                read_section_key("flux-capture", &encoded, 0, &known).expect("what was written");

            assert_eq!(&decoded, key);
            assert_eq!(used, encoded.len(), "the key states its own extent");
        }
    }

    #[test]
    fn an_index_stating_an_unreduced_position_is_refused() {
        // A written key is always reduced, so 74/4 in an index is
        // corruption. It matters more than it looks: reducing it here
        // would silently collide with the 37/2 already indexed, and
        // one of the two sections would become unreachable while the
        // other answered for it.
        let mut encoded = Vec::new();
        write_varint(&mut encoded, "c1541".len() as u64);
        encoded.extend_from_slice(b"c1541");
        write_varint(&mut encoded, 74);
        write_varint(&mut encoded, 4);
        for value in [0, 0, 0, 0] {
            write_varint(&mut encoded, value);
        }

        let error = read_section_key("flux-capture", &encoded, 0, &["c1541"]).unwrap_err();

        assert_eq!(error.category(), ErrorCategory::InvalidImage);
    }

    #[test]
    fn an_index_naming_a_namespace_the_reader_does_not_know_is_refused() {
        // The backing is private session state, so a namespace the
        // reader cannot place means the index is not the one this
        // layer wrote. Refusing beats resolving it to something
        // plausible and addressing the wrong sections thereafter.
        let key = SectionKey::new(
            TrackKey::new("kryoflux", 36, 0),
            ScopeId::Track,
            SectionKind::TrackMetadata,
            0,
        );
        let mut encoded = Vec::new();
        write_section_key(&mut encoded, &key);

        let error = read_section_key("flux-capture", &encoded, 0, &["scp"]).unwrap_err();

        assert_eq!(error.category(), ErrorCategory::InvalidImage);
        assert!(error.to_string().contains("kryoflux"), "{error}");
    }

    #[test]
    fn a_section_whose_bytes_changed_under_the_index_is_refused() {
        // The index states what the record should check to. A backing
        // that answered anyway would hand back flux that never existed.
        let track = TrackKey::new("kryoflux", 36, 0);
        let mut writer = SectionWriter::new();
        let (key, _, payload) = transition_section(&track, 0);
        writer.append(key.clone(), payload).expect("keys ascend");
        let mut bytes = writer.finish();
        let total = bytes.len() as u64;

        bytes[2] ^= 0x01;

        let error = read_section(&CountingSource::new(bytes), total, &key, KNOWN).unwrap_err();
        assert_eq!(error.category(), ErrorCategory::InvalidImage);
    }

    #[test]
    fn sections_appended_out_of_key_order_are_refused() {
        // Sections are emitted in key order so the index can be built
        // and searched in it; accepting them out of order would make
        // the ordering a hope rather than a property.
        let track = TrackKey::new("kryoflux", 36, 0);
        let mut writer = SectionWriter::new();
        let (second, _, second_payload) = transition_section(&track, 1);
        let (first, _, first_payload) = transition_section(&track, 0);

        writer
            .append(second, second_payload)
            .expect("the first append is in order");
        let error = writer.append(first, first_payload).unwrap_err();

        assert_eq!(error.category(), ErrorCategory::InvalidImage);
    }
}
