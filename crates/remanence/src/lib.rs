// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! Self-contained disk image analysis library.
//!
//! A [`Session`] holds two things — the devices, which are
//! configuration, and the media, which are state — and **the
//! medium is the content handle**: [`Medium`] is the node a caller
//! holds, and everything a recording can answer answers on it.
//!
//! [`Session::load_media`] is the declared reading: one declared
//! source and one concrete [`Format`], checked by that format's own
//! adapter and refused by name where the evidence cannot bear it.
//! **The source takes one of four shapes** ([`MediaSource`], arrived
//! at by plain conversion): the caller's own opened [`std::fs::File`],
//! a collection of them, one [`FileSource`] taken from another
//! medium's namespace ([`File::source`], [`StorageSpace::files`]), or
//! a collection of those — and **a format declares which shape it
//! reads**, a KryoFlux capture set being a declared collection and
//! every other claimed format one artifact. **The declaration carries
//! the device its content was recorded by** — bare where the format
//! records one ([`Format::H8d`] is a Heathkit H-17 recording), stated
//! by the caller where it records several ([`Format::Qcow2`] and its
//! `device: HardDrive`, [`Format::KryoFlux`] and its
//! `device: FloppyDrive`) — and a [`Medium`] answers
//! [`Medium::device_type`] with it, beside the [`Medium::article`]
//! that says what the substrate is. **Whoever opens owns the lock** (P7 as amended) — that open
//! is the claim, the library checks it for exactly one thing (may it
//! write?), honours the answer exactly, and takes no lock of its own; a
//! name is recovered from the handle for location alone, under an
//! identity check, and a handle this host cannot name refuses the two
//! location-dependent journeys by name and serves everything else.
//!
//! **Devices are configuration beside that, and linking is the one
//! edge.** [`Session::add_device`] takes a [`DeviceSlot`] — a
//! [`DeviceType`] as concrete as the drive the machine actually had, or
//! the archive receiver — [`DeviceView::insert`] links a pooled medium
//! into it by **device-type equality**, refusing a medium another device
//! recorded and naming both sides, and [`DeviceView::eject`] **severs
//! only**, the claim and everything buffered surviving in the pool.
//! [`Session::release_media`] is the one state-destroying verb.
//!
//! **Every pool runs the same three verbs: create, look up, release.**
//! A lookup — [`Session::device`], [`Session::medium`] and their
//! `_mut` forms — answers with an
//! `Option`, absence being an answer rather than a manufactured error,
//! and there is no `require_*` form: a caller who wants a demand writes
//! it, where they know what the absence means. Creation still refuses by
//! name, and so do the removals, which are all spelled `release_*` —
//! [`Session::release_device`] ejecting first, and
//! [`Session::release_media`] severing its own link before it ends the
//! claim.
//!
//! [`discover_media`] answers the other question — *what is this?* —
//! on no handle at all: the exact medium, the device families served it,
//! and the image format's declared default device. It opens the artifact
//! by name, so P7's mandatory denial applies there in full, and it
//! **builds no cache**: no medium, no session cache, no spilled
//! backing, because a cache bound is the load's declaration and a verb
//! that creates nothing has nothing to bound (P27). The
//! [`Discovery`] it returns holds that claim and the work already done,
//! and [`Session::load_discovery`] consumes it into the pool so nothing
//! runs twice — the plain door, opening where the recognizing format
//! records one device type, with [`Session::load_discovery_as`] taking
//! the caller's declaration where it records several.
//! [`Session::add_device_for`] composes the acts over a discovery
//! that knows what recorded it, and refuses by name where none does.
//!
//! On the medium: [`Medium::identify`] reports the layers of the
//! artifact's nesting (archive, image, physical media, filesystem)
//! recognized by built-in executable adapters, over the image's own
//! bytes, while [`Medium::inspect`] works over the disk a format adapter
//! presents above them.
//!
//! **Content is reached through the partition that composes it.**
//! [`Medium::partition`] answers by the scheme's own ordinal,
//! [`Medium::partitions`] is the pool and [`Medium::partition_scheme`]
//! the scheme it was populated under — all of it established at the load
//! and evidence from then on, never probed for on demand. A medium
//! recording no scheme bears the **direct partition** at ordinal 0: the
//! library's own composition of the whole content, carried as provenance
//! and never as evidence. A [`Partition`] states its raw type value
//! beside a reading of what that value declares, whether the scheme
//! flags it active, and [`PartitionView::check_type`] checks a caller's own
//! reading against the recorded byte.
//!
//! **Authorship is the third fact class, and it creates media whole.**
//! Evidence is discovered onto media and declarations are configured
//! onto devices; [`Session::new_media`] is neither.
//! It takes one enumerated [`NewMedia`] kind — the **blank article
//! kinds**, each naming one article of the catalog and creating that
//! manufactured substrate with nothing recorded on it, and
//! [`NewMedia::ChsDisk`], whose content is addressed in the cylinders,
//! heads and sectors the author states — and the facts that declaration
//! carries become the medium's original facts: its provenance, and its
//! [`Medium::geometry`], whose one reading is
//! [`GeometrySource::Authorship`]. **An authored blank assumes no
//! device**: [`Medium::device_type`] answers `None`, no drive takes one,
//! and the arc from authored to recorded is reserved. It is
//! session-backed until an explicit encode gives it an artifact, and
//! [`Medium::commit`] is the ordinary commit point over it.
//!
//! **Geometry is discovered, and the sector verbs address in it.**
//! [`Medium::geometry`] is what the sources beneath the medium stated
//! about the recording's coordinates — the format's own declaration, a
//! FAT boot record's recorded heads and sectors-per-track, the partition
//! table's end tuples, arithmetic over the content's extent — each
//! reading kept with where it came from. Sources that disagree settle
//! nothing: the answer is [`GeometryState::Undetermined`], carrying both
//! readings. [`Medium::read_sector`] and [`Medium::write_sector`] address
//! in the coordinates that establishes, on the types whose
//! [`DeviceType::addressing`] says the recording is sector-addressed,
//! and refuse by name — their own [`GeometryRule`] set — everywhere
//! else. A write buffers until [`Medium::commit`] like every other
//! write, and **nothing is ever declared onto a medium that exists**.
//!
//! **File access lives on one node.** The vantage doors —
//! [`PartitionView::volume`] and [`PartitionView::filesystem`], each
//! `Option` — hand out the one [`StorageSpace`] the partition composes,
//! and the verbs ([`StorageSpace::entries`], [`StorageSpace::stat`],
//! [`StorageSpace::get_file`] and their kin) live there and nowhere
//! else. Both doors are lookups, because everything behind them was
//! specified and verified: the namespace opens under the declared
//! partition type where one determines it, and under
//! [`PartitionView::filesystem_as`] — the caller's reading, the
//! library's check — where nothing does.
//!
//! **An archive is a medium like any other.** It is loaded by its
//! declared grammar ([`Format::Zip`], [`Format::SevenZip`]), may be
//! seated in the archive receiver ([`DeviceSlot::Archive`]) — which is
//! no device type, an archive having been recorded by no device — and
//! its content is walked through
//! the namespace door of the direct partition it bears — a namespace
//! with no addressed extent beneath it. An entry recognized as an
//! artifact of its own is opened by [`File::discover`] and becomes a
//! medium of its own, which is the one recursion this model has.
//!
//! **The flux family's physical stratum is reached through its own
//! type.** [`FluxImage`] opens a `.remanence` artifact and answers
//! the physical facts of one disk, and the C64 renditions are mastered
//! off it: [`FluxImage::write_d64`], [`FluxImage::write_g64`]
//! and [`FluxImage::write_p64`], each paired with a `describe_`
//! verb that computes everything and writes nothing, and each stating
//! what its destination did not carry (P29).
//!
//! **A flux artifact loads as a medium like any other** (F59). A
//! KryoFlux capture set is the collection-sourced format:
//! `Format::KryoFlux { device }` over a declared collection checks the
//! member grammar, the set's completeness, every stream's own grammar
//! and the declared device's profile claim whole, runs the gap-first
//! reduction under the profile's declared materialization defaults —
//! every revolution of every location aligned by gap correspondence,
//! the cell lattice measured from the intervals themselves, the angles
//! integrated gap-first, and the fat track merged under measured
//! agreement — and pools a medium of the declared family with the
//! verdicts, the policy and the declared-loss account as provenance
//! ([`Medium::assurance`]). [`Format::P64`] loads the served form
//! straight in: a P64 already holds a flux medium at rest.
//!
//! **A flux medium is then read the way a drive reads it**, and the
//! type carries the rules (P30 reached through the type):
//! [`Medium::bitstream`] clocks the family's pulses into a
//! [`Bitstream`] under the profile's declared channel,
//! [`Medium::bytestream`] resolves that into the [`Bytestream`]
//! the family's declared group code makes of it — argument-free both,
//! because being a `Commodore1541` medium *means* reading through the
//! c1541 channel and codec — and
//! [`Bytestream::location`] serves the framed bytes of one
//! [`Location`]. The same rungs stand on a `.remanence` image through
//! [`FluxImage::materialize_c1541_bitstream`] and
//! [`Bitstream::materialize_bytestream`]. The rungs assign
//! nothing above a byte; [`Bytestream::recognize_sectors`]
//! is where that ends, and it ends by stating what it derives — every
//! record carrying its evidence, and [`C1541Sectors::read_sector`]
//! refusing by name (its own [`SectorRule`] set) rather than filling in
//! a block the recording does not hold. The filesystem door is the
//! medium's own: the direct partition a flux medium bears at ordinal 0,
//! whose `filesystem_as("cbmdos")` answers the same [`StorageSpace`] a
//! disk image's partition does, because the file verbs live on the
//! namespace and on nothing else — carrying the BAM header as its
//! label, the directory in the order it was written, and a size
//! established by walking each file's chain.
//! [`C1541Sectors::partition`] is the same composition over a layer
//! reached through the `.remanence` root's own ladder.
//!
//! Every open also states what it established about the evidence beneath
//! it ([`Medium::assurance`]): a source short of what its own
//! interpretation declares is read as far as it truthfully goes,
//! read-only, with the shortfall named rather than hidden or thrown away
//! whole (P28).

// The crate's own strata, outermost first. Each group's `mod.rs` says
// what its seam is and which principles govern it; `AGENTS.md` maps the
// same eight groups onto the architecture.
mod archive;
mod checksum;
mod codec;
mod error;
mod evidence;
mod filesystem;
mod flux;
mod image;
mod io;
mod model;
mod partition;

pub use crate::filesystem::dos_name::DosNameRule;
pub use crate::filesystem::fat::FatKind;
pub use crate::filesystem::{Entry, EntryFact, EntryKind, File, SpaceRule, StorageSpace};
pub use crate::flux::c1541::renditions::{D64Block, D64Report, G64HalfTrack, G64Report};
pub use crate::flux::c1541::sectors::{
    C1541Sectors, ContestedAddress, SectorClaim, SectorLocation, SectorReport, SectorRule,
};
pub use crate::flux::p64::{P64HalfTrack, P64Report};
pub use crate::flux::presentation::{
    Bitstream, BitstreamLocation, BitstreamReport, Bytestream, BytestreamLocation,
    BytestreamReport, Location, LocationBytes,
};
pub use crate::flux::remanence::format::FluxWriteReport;
pub use crate::flux::remanence::image::{FluxHole, FluxImage, FluxImageReport, FluxOrbit};
pub use crate::io::cache::DEFAULT_CACHE_BYTES;
pub use crate::io::device::{AccessIntent, AccessMode, Claim};
pub use crate::io::source::FileSource;
pub use crate::model::assurance::{Assurance, AssuranceCondition, AssuranceOutcome, ByteRange};
pub use crate::model::authored::{NewMedia, NewMediaClaim};
pub use crate::model::device_type::{DeviceSlot, DeviceType, FloppyDrive, HardDrive, OpticalDrive};
pub use crate::model::discovery::{Discovery, discover_media};
pub use crate::model::disk::DiskFormat;
pub use crate::model::geometry::{
    Geometry, GeometryReading, GeometryRule, GeometrySource, GeometryState, RecordingGeometry,
};
pub use crate::model::media::{Format, FormatClaim, MediaId, MediaSource, Medium};
pub use crate::model::pools::Session;
pub use crate::model::report::{
    DeclaredGeometry, DeviceInfo, DiskContent, DiskReport, FilesystemId, FilesystemInfo,
    LabelReading, PartitionSchemaInfo, RegionId, RegionInfo, RegionRole, VolumeId, VolumeInfo,
    VolumeLabel, VolumeOrigin,
};
pub use crate::model::session::{
    ArchiveLayout, DiskLayout, FilesystemLayout, Identification, ImageLayout, Layer, LayerKind,
    LayerLayout, PhysicalMediaLayout, SectorLayout, SizeInformation, TrackSectorLayout,
};
pub use crate::model::storage_device::{AttachmentId, DeviceView, StorageDevice};
pub use crate::partition::{
    Partition, PartitionRule, PartitionScheme, PartitionType, PartitionView,
};
pub use error::{Error, ErrorCategory, Result, RuleIdentity};
pub use evidence::DeclaredLoss;
