// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! Self-contained disk image analysis library.
//!
//! A [`Session`] owns two pools — the [`Machine`]s that hold devices,
//! which are configuration, and the media, which are state — and **the
//! medium is the content handle**: [`Medium`] is the node a caller
//! holds, and everything a recording can answer answers on it.
//!
//! [`Session::load_media`] is the declared reading: the caller's own
//! opened [`std::fs::File`] and one concrete [`Format`], checked by that
//! format's own adapter and refused by name where the evidence cannot
//! bear it. **The declaration carries the device its content was
//! recorded by** — bare where the format records one ([`Format::H8d`]
//! is a Heathkit H-17 recording), stated by the caller where it records
//! several ([`Format::Qcow2`] and its `device: HardDrive`) — and a
//! [`Medium`] answers [`Medium::device_type`] with it, beside the
//! [`Medium::article`] that says what the substrate is. **Whoever opens owns the lock** (P7 as amended) — that open
//! is the claim, the library checks it for exactly one thing (may it
//! write?), honours the answer exactly, and takes no lock of its own; a
//! name is recovered from the handle for location alone, under an
//! identity check, and a handle this host cannot name refuses the two
//! location-dependent journeys by name and serves everything else.
//!
//! **Devices are configuration beside that, and linking is the one
//! edge.** [`MachineView::add_device`] takes a [`DeviceSlot`] — a
//! [`DeviceType`] as concrete as the drive the machine actually had, or
//! the archive receiver — [`DeviceView::insert`] links a pooled medium
//! into it by **device-type equality**, refusing a medium another device
//! recorded and naming both sides, and [`DeviceView::eject`] **severs
//! only**, the claim and everything buffered surviving in the pool.
//! [`Session::release_media`] is the one state-destroying verb.
//!
//! **Every pool runs the same three verbs: create, look up, release.**
//! A lookup — [`Session::machine`], [`Session::device`],
//! [`Session::medium`] and their `_mut` forms — answers with an
//! `Option`, absence being an answer rather than a manufactured error,
//! and there is no `require_*` form: a caller who wants a demand writes
//! it, where they know what the absence means. Creation still refuses by
//! name, and so do the removals, which are all spelled `release_*` —
//! [`Session::release_machine`] cascading through the configuration
//! below it, [`MachineView::release_device`] ejecting first, and
//! [`Session::release_media`] severing its own link before it ends the
//! claim.
//!
//! [`discover_media`] answers the other question — *what is this?* —
//! on no handle at all: the exact medium, the device families served it,
//! and the image format's declared default device. It opens the artifact
//! by name, so P7's mandatory denial applies there in full. The
//! [`Discovery`] it returns holds that claim and the work already done,
//! and [`Session::load_discovery`] consumes it into the pool so nothing
//! runs twice — the plain door, opening where the recognizing format
//! records one device type, with [`Session::load_discovery_as`] taking
//! the caller's declaration where it records several.
//! [`MachineView::add_device_for`] composes the acts over a discovery
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
//! flags it active, and [`PartitionView::as_type`] checks a caller's own
//! reading against the recorded byte.
//!
//! **Geometry is discovered, and the sector verbs address in it.**
//! [`Medium::geometry`] is what the sources beneath the medium stated
//! about the recording's coordinates — the format's own declaration, a
//! FAT boot record's recorded heads and sectors-per-track, the partition
//! table's end tuples, arithmetic over the content's extent — each
//! reading kept with where it came from. Sources that disagree settle
//! nothing: the answer is [`GeometryState::Undetermined`], carrying both
//! readings. [`Medium::get_sector`] and [`Medium::put_sector`] address
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
//! type.** [`RemanenceImage`] opens a `.remanence` artifact and answers
//! the physical facts of one disk, and the C64 renditions are mastered
//! off it: [`RemanenceImage::write_d64`], [`RemanenceImage::write_g64`]
//! and [`RemanenceImage::write_p64`], each paired with a `describe_`
//! verb that computes everything and writes nothing, and each stating
//! what its destination did not carry (P29).
//!
//! **A capture reduces to one of those images.**
//! [`CaptureSet::plan_reconstruction`] takes a declared
//! [`ReconstructionPolicy`] and computes the whole gap-first reduction
//! without writing anything — every revolution of every location
//! aligned by gap correspondence, the cell lattice measured from the
//! intervals themselves, the angles integrated gap-first, and the fat
//! track merged under measured agreement. [`ReconstructionPlan::report`]
//! is that reduction stated whole, and [`ReconstructionPlan::execute`]
//! answers with the [`RemanenceImage`] itself rather than a root of its
//! own.
//!
//! **An image, or the medium a P64 holds, is then read the way a drive
//! reads it**, one declared rung at a time:
//! [`RemanenceImage::materialize_c1541_bitstream`] clocks the family's
//! pulses into a [`C1541Bitstream`],
//! [`C1541Bitstream::materialize_c1541_bytestream`] resolves that into
//! the [`C1541Bytestream`] a declared group code makes of it, and
//! [`C1541Bytestream::recognize_c1541_sectors`] reads the recording's
//! own records out of those bytes under the family's declared grammar.
//! The two lower rungs assign nothing above a byte; the third is where
//! that ends, and it ends by stating what it derives — every record
//! carrying its evidence, and [`C1541Sectors::read_sector`] refusing by
//! name (its own [`SectorRule`] set) rather than filling in a block the
//! recording does not hold. [`C1541Sectors::partition`] is the rung
//! above that: the direct partition over the recording, whose
//! `filesystem_as("cbmdos")` answers the same [`StorageSpace`] a disk
//! image's partition does, because the file verbs live on the namespace
//! and on nothing else — carrying the BAM header as its label, the
//! directory in the order it was written, and a size established by
//! walking each file's chain.
//!
//! Every open also states what it established about the evidence beneath
//! it ([`Medium::assurance`]): a source short of what its own
//! interpretation declares is read as far as it truthfully goes,
//! read-only, with the shortfall named rather than hidden or thrown away
//! whole (P28).

mod adapters;
mod archive;
mod assurance;
mod c1541_presentation;
mod c1541_sectors;
mod c64_renditions;
mod cache;
mod cbm_dos;
mod checksum;
mod deflate;
mod device;
mod device_type;
mod discovery;
mod disk;
mod dos_letters;
mod dos_name;
mod drive_profile;
mod encoded_bytestream;
mod error;
mod evidence;
mod fat;
mod filesystem;
mod filesystem_catalog;
mod flux_analysis;
mod flux_capture;
mod flux_medium;
mod geometry;
mod handle;
mod hardware_bitstream;
mod hdos;
mod inflate;
mod journal;
mod kryoflux;
mod lzma;
mod machine;
mod mbr;
mod media;
mod media_profile;
mod p64;
mod partition;
mod qcow2;
mod remanence_format;
mod remanence_image;
mod remanence_reconstruction;
mod report;
mod session;
mod sevenzip;
mod source;
mod storage_device;
mod vdi;
mod volume;
mod zip;

pub use assurance::{Assurance, AssuranceCondition, AssuranceOutcome, ByteRange};
pub use c64_renditions::{D64Block, D64Report, G64HalfTrack, G64Report};
pub use c1541_presentation::{
    AlignmentPolicy, BitstreamLocation, BitstreamReport, BytestreamLocation, BytestreamReport,
    C1541Bitstream, C1541Bytestream, DensityPolicy, GcrCodecPolicy, ReadChannelPolicy,
    UnassignedSymbolPolicy, UnzonedPolicy, WeakPulsePolicy,
};
pub use c1541_sectors::{
    C1541Sectors, ChecksumFailurePolicy, ContestedAddress, SectorClaim, SectorLocation,
    SectorPolicy, SectorReport, SectorRule, UnpairedRecordPolicy,
};
pub use cache::DEFAULT_CACHE_BYTES;
pub use device::{AccessIntent, AccessMode, Claim};
pub use device_type::{DeviceSlot, DeviceType, FloppyDrive, HardDrive};
pub use discovery::{Discovery, discover_media, discover_media_with_cache};
pub use disk::DiskFormat;
pub use dos_letters::{
    DosAssignmentRule, DosMachine, DriveMap, DriveMapping, LetterOutcome, MachineDevice,
    ResidentCondition,
};
pub use dos_name::DosNameRule;
pub use drive_profile::{LocationVerdict, ProfileVerdict, Recognition, ZoneClaim};
pub use error::{Error, ErrorCategory, Result, RuleIdentity};
pub use evidence::DeclaredLoss;
pub use fat::FatKind;
pub use filesystem::{Entry, EntryFact, EntryKind, File, SpaceRule, StorageSpace};
pub use geometry::{
    Geometry, GeometryReading, GeometryRule, GeometrySource, GeometryState, RecordingGeometry,
};
pub use kryoflux::{
    CaptureIssue, CaptureRunReport, CaptureSet, CaptureSetMember, CaptureSetReport,
    ObservationReport, StepPosition, TimeBaseReport,
};
pub use machine::{Machine, MachineView, Session};
pub use media::{Format, FormatClaim, MediaId, Medium};
pub use p64::{P64HalfTrack, P64Image, P64Report};
pub use partition::{Partition, PartitionRule, PartitionScheme, PartitionType, PartitionView};
pub use remanence_format::RemanenceWriteReport;
pub use remanence_image::{RemanenceHole, RemanenceImage, RemanenceImageReport, RemanenceOrbit};
pub use remanence_reconstruction::{
    ReconstructedOrbit, ReconstructionPlan, ReconstructionPolicy, ReconstructionReport,
    RecordingSelection,
};
pub use report::{
    DeclaredGeometry, DeviceInfo, DiskContent, DiskReport, FilesystemId, FilesystemInfo,
    LabelReading, PartitionSchemaInfo, RegionId, RegionInfo, RegionRole, VolumeId, VolumeInfo,
    VolumeLabel, VolumeOrigin,
};
pub use session::{
    ArchiveLayout, DiskLayout, FilesystemLayout, Identification, ImageLayout, Layer, LayerKind,
    LayerLayout, PhysicalMediaLayout, SectorLayout, SizeInformation, TrackSectorLayout,
};
pub use storage_device::{AttachmentId, DeviceView, StorageDevice};
