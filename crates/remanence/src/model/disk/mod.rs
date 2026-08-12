// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The media state a storage device homes (U3 and U4): a raw, qcow2 or
//! VDI image open under the P7 claim, its partitions and volumes as they
//! actually are, and read/write access to the files in its FAT volumes
//! with a commit point (P2) — everything rolls back until `commit`. The commit is
//! durable (P9): a recovery journal is armed beneath the write-through,
//! so an interruption at any point leaves state the next open
//! reconciles — wholly the old image or wholly the committed new one —
//! before the disk is exposed. An image whose content lives partly
//! behind it — a qcow2 backing chain, a VDI differencing chain — opens
//! as one composed disk (U6), every member claimed for the session's
//! life and writes allocated copy-on-write into the top image only.
//!
//! Every open carries its assurance (P28). A source that satisfies its
//! interpretation is verified and keeps whatever authority the caller
//! declared; one that falls short of it — a raw image whose FAT boot
//! record declares more bytes than the file holds — is degraded: bounded
//! to the extent that is really there, read-only for the session's whole
//! life, and naming every operation it withholds.
//!
//! **This is not a handle.** A caller never holds a medium outside a
//! device, so the medium survives as a model node and as data on
//! [`crate::StorageDevice`] rather than as a type of its own. Every verb
//! below is reached through the device that homes it, and the
//! caller-facing contract each one answers for is documented there,
//! beside the slot-side facts it sits with.

mod commit;
mod files;
mod recognition;
mod state;

#[cfg(test)]
mod fixtures;

pub(crate) use recognition::MediumRecognition;
pub(crate) use state::MediaState;

use recognition::{FLUX_RECORDED_DEVICES, no_space};

use std::path::Path;

use crate::archive::{ArchiveMedium, ArchiveRecognition};
use crate::error::{Error, Result};
use crate::flux::load::{self as flux_load, CollectionMember, FluxState};
use crate::io::device::AccessMode;
use crate::io::source::{self};
use crate::model::assurance::Assurance;
use crate::model::authored::{AuthoredMedium, AuthoredSpace, NewMedia};
use crate::model::device_type::{DeviceSlot, DeviceType};
use crate::model::geometry::Geometry;
use crate::model::media::{FluxFormat, Format, MediaSource, SourceShape};
use crate::model::media_profile::MediaProfile;
use crate::model::session::Identification;
use crate::partition::PartitionPool;

/// The image container format a disk image turned out to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiskFormat {
    Raw,
    Qcow2 { version: u32 },
    Vdi { major: u32, minor: u32 },
}

/// What occupies a device's slot: a medium of one of the two vantages
/// the model claims.
///
/// **Families own their representation** (P14), and this is that rule at
/// the state tier. A space-native medium holds an addressed disk with
/// structure above it; a namespace-native one holds named entries and no
/// space at all. Rather than one state with most of its fields empty,
/// each kind is its own, and the verbs that need a space say so —
/// asking an archive to inspect partitions is a category error answered
/// by name, not a hole to fall into.
#[derive(Debug)]
pub(crate) enum MediumState {
    /// A medium whose native vantage is a space: the block state an
    /// image format loaded, with partitions, volumes and namespaces
    /// above it.
    Space(MediaState),
    /// A medium whose native vantage is a namespace: an archive, whose
    /// content is names and whose bytes are its encoding (P13).
    Archive(ArchiveMedium),
    /// A flux medium: the served form — one circular pulse stream per
    /// family-addressed location — with the presentation ladder above
    /// it and no byte-addressed space at all (P13). It arrives through
    /// the two flux-family declarations: a KryoFlux collection reduced
    /// under the profile's declared defaults, and a P64 loaded straight
    /// in (F59).
    Flux(FluxState),
    /// A medium the author created whole (F60): no artifact beneath it,
    /// the author's own facts as its original facts, and no device
    /// assumed. It is the third fact class arriving at the state tier —
    /// authorship, beside the evidence the three above are read from —
    /// and the same rule shapes it: an authored blank whose kind states
    /// coordinates bears the content they address, and one that is a
    /// blank article bears none.
    Authored(AuthoredMedium),
}

impl MediumState {
    /// Loads the caller's declared source as the format they
    /// **declared** it to be.
    ///
    /// This is the declared reading, and the declaration is checked
    /// rather than trusted: the one adapter the format names is asked
    /// whether the evidence bears it, and a refusal names both what was
    /// declared and what was found. Nothing here probes for a second
    /// answer — a caller who does not know what an artifact is asks
    /// [`discover_media`](crate::discover_media) instead.
    ///
    /// **The declaration is checked against itself first.** A format
    /// paired with a device type its adapter does not record is refused
    /// before a byte is read, because that refusal is about the
    /// declaration rather than about the artifact: reading the evidence
    /// first would answer a question nobody could act on, and would
    /// report a blank image as an invalid qcow2 when what was wrong was
    /// the pairing (P3, P6). The **source shape is part of the same
    /// declaration** (F59): a format declares whether it reads one
    /// artifact or a collection, and a shape it does not read is
    /// refused by name before anything else runs.
    pub(crate) fn load(source: MediaSource, format: Format, cache_bytes: u64) -> Result<Self> {
        format.check_pairing()?;
        let MediaSource(shape) = source;
        let claim = format.claim();
        let collection_offered = matches!(shape, SourceShape::Handles(_) | SourceShape::Entries(_));
        if claim.takes_collection() != collection_offered {
            return Err(Error::unsupported(format!(
                "the {} reads {}, and the load offered {}",
                claim.name(),
                if claim.takes_collection() {
                    "a declared collection of sources — one disk spread over a \
                     stream per head per step position"
                } else {
                    "one artifact"
                },
                shape.describe()
            )));
        }
        if let Some(flux) = format.flux_family() {
            return match flux {
                FluxFormat::KryoFlux { device } => {
                    let members = match shape {
                        SourceShape::Handles(files) => {
                            files.into_iter().map(CollectionMember::Handle).collect()
                        }
                        SourceShape::Entries(entries) => {
                            entries.into_iter().map(CollectionMember::Entry).collect()
                        }
                        SourceShape::Handle(_) | SourceShape::Entry(_) => {
                            unreachable!("the shape check admitted a collection")
                        }
                    };
                    Ok(Self::Flux(FluxState::load_kryoflux(
                        members,
                        device,
                        cache_bytes,
                    )?))
                }
                FluxFormat::P64 => {
                    let claimed = match shape {
                        SourceShape::Handle(file) => source::claim_handle(file)?,
                        SourceShape::Entry(entry) => entry.claim(),
                        SourceShape::Handles(_) | SourceShape::Entries(_) => {
                            unreachable!("the shape check admitted one artifact")
                        }
                    };
                    let path = claimed
                        .source_path
                        .as_deref()
                        .map(|path| path.display().to_string());
                    let claim = claimed.claim_class;
                    let source = claimed.resolve(cache_bytes);
                    Ok(Self::Flux(FluxState::load_p64(
                        &source,
                        path,
                        claim,
                        cache_bytes,
                    )?))
                }
            };
        }
        if let Some(grammar) = format.archive_grammar() {
            return match shape {
                SourceShape::Handle(file) => Ok(Self::Archive(
                    ArchiveRecognition::load(file, grammar)?.into_medium(cache_bytes),
                )),
                SourceShape::Entry(entry) => Err(Error::unsupported(format!(
                    "this release reads an archive from the caller's own opened \
                     file: '{}' was reached through another medium's namespace, \
                     and a nested archive is not claimed",
                    entry.name()
                ))),
                SourceShape::Handles(_) | SourceShape::Entries(_) => {
                    unreachable!("the shape check admitted one artifact")
                }
            };
        }
        match shape {
            SourceShape::Handle(file) => {
                Ok(Self::Space(MediaState::load(file, format, cache_bytes)?))
            }
            SourceShape::Entry(entry) => Ok(Self::Space(MediaState::load_claimed(
                entry.claim(),
                format,
                cache_bytes,
            )?)),
            SourceShape::Handles(_) | SourceShape::Entries(_) => {
                unreachable!("the shape check admitted one artifact")
            }
        }
    }

    /// Creates the medium an author declared, whole — the third fact
    /// class (F60).
    ///
    /// Nothing is read, nothing is probed and nothing is opened: there is
    /// no artifact yet. The declaration is checked against itself — a
    /// kind this release does not author, and coordinates that address
    /// nothing, are refused by name — and what it states becomes the
    /// medium's original facts.
    pub(crate) fn authored(kind: NewMedia, cache_bytes: u64) -> Result<Self> {
        Ok(Self::Authored(AuthoredMedium::create(kind, cache_bytes)?))
    }

    /// The artifact claimed — the archive itself for an image loaded out
    /// of one — where a name for it exists.
    pub(crate) fn path(&self) -> Option<&str> {
        match self {
            Self::Space(space) => space.path(),
            Self::Archive(archive) => archive.path(),
            Self::Flux(flux) => flux.path(),
            // An authored medium has no artifact to name and no handle a
            // name could be recovered from.
            Self::Authored(_) => None,
        }
    }

    /// The artifact as a refusal names it: the recovered name, the stated
    /// fact that the caller's handle has none, or — where nothing was
    /// opened at all — what the author made.
    pub(crate) fn named(&self) -> String {
        match self {
            Self::Authored(authored) => authored.named(),
            _ => crate::model::media::named(self.path()),
        }
    }

    /// The resolved artifact — the entry name for an image loaded out of
    /// an archive, else the source's own name.
    pub(crate) fn image_path(&self) -> Option<&Path> {
        match self {
            Self::Space(space) => space.image_path(),
            Self::Archive(archive) => archive.path().map(Path::new),
            Self::Flux(flux) => flux.path().map(Path::new),
            Self::Authored(_) => None,
        }
    }

    /// The artifact's own bytes, where the medium came from one — zero
    /// for an authored medium, which came from nowhere but its author.
    pub(crate) fn image_size_bytes(&self) -> u64 {
        match self {
            Self::Space(space) => space.image_size_bytes(),
            Self::Archive(archive) => archive.size_bytes(),
            Self::Flux(flux) => flux.source_bytes(),
            Self::Authored(_) => 0,
        }
    }

    /// Reads the artifact's own bytes, streamed (P27).
    pub(crate) fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<()> {
        match self {
            Self::Space(space) => space.read_at(offset, buf),
            Self::Archive(archive) => archive.read_at(offset, buf),
            // A flux medium's evidence stays behind the surface: there
            // is no public flux, pulse, or capture-run iterator, and no
            // byte plane either — the collection has no one artifact,
            // and the reading of one is the presentation ladder's.
            Self::Flux(flux) => Err(flux_load::no_space("read_at", flux)),
            // And an authored medium has no artifact plane at all: its
            // content is what the author created, reached in the
            // coordinates they stated.
            Self::Authored(authored) => Err(authored.no_image("read_at")),
        }
    }

    pub(crate) fn media(&self) -> &'static MediaProfile {
        match self {
            Self::Space(space) => space.media(),
            Self::Archive(archive) => archive.media(),
            Self::Flux(flux) => flux.media(),
            Self::Authored(authored) => authored.media(),
        }
    }

    /// The device this medium's content is assumed recorded by, where a
    /// declaration named one.
    ///
    /// `None` has two readings and they are the same reading: an archive
    /// was recorded by no device, and a space-native medium reached
    /// without a declaration — a discovery over a format that records
    /// several device types — carries none until a load names it. The
    /// pool refuses to admit the second (P3): a medium that cannot say
    /// what recorded it cannot be seated or laid out.
    pub(crate) fn device_type(&self) -> Option<DeviceType> {
        match self {
            Self::Space(space) => space.device_type(),
            // An archive was recorded by no device, and neither was an
            // authored blank: authorship assumes none, and only the
            // reserved authored-to-recorded arc would bind one.
            Self::Archive(_) | Self::Authored(_) => None,
            Self::Flux(flux) => Some(flux.device_type()),
        }
    }

    /// What slot this medium goes in, where it goes in one at all — the
    /// recording side of the insert check.
    ///
    /// An authored medium answers `None` and it is not a gap: no drive
    /// takes a blank nobody recorded, so the edge refuses by name rather
    /// than seating it somewhere.
    pub(crate) fn slot(&self) -> Option<DeviceSlot> {
        match self {
            Self::Space(space) => space.device_type().map(DeviceSlot::Recorded),
            Self::Archive(_) => Some(DeviceSlot::Archive),
            Self::Flux(flux) => Some(DeviceSlot::Recorded(flux.device_type())),
            Self::Authored(_) => None,
        }
    }

    /// Whether this medium is a reading that could not say what recorded
    /// it — the discovery over a format admitting several device types,
    /// which the pool refuses to admit (P3).
    ///
    /// It is not the same question as [`MediumState::slot`]: an authored
    /// medium has no device type either, and there is nothing missing
    /// about it.
    pub(crate) fn undeclared(&self) -> bool {
        matches!(self, Self::Space(space) if space.device_type().is_none())
    }

    /// The authored medium this is, where it is one.
    pub(crate) fn authored_medium(&self) -> Option<&AuthoredMedium> {
        match self {
            Self::Authored(authored) => Some(authored),
            Self::Space(_) | Self::Archive(_) | Self::Flux(_) => None,
        }
    }

    pub(crate) fn mode(&self) -> AccessMode {
        match self {
            Self::Space(space) => space.mode(),
            Self::Archive(archive) => archive.mode(),
            Self::Flux(flux) => flux.mode(),
            Self::Authored(authored) => authored.mode(),
        }
    }

    pub(crate) fn assurance(&self) -> &Assurance {
        match self {
            Self::Space(space) => space.assurance(),
            Self::Archive(archive) => archive.assurance(),
            Self::Flux(flux) => flux.assurance(),
            Self::Authored(authored) => authored.assurance(),
        }
    }

    pub(crate) fn identify(&self) -> Identification {
        match self {
            Self::Space(space) => space.identify(),
            Self::Archive(archive) => archive.identify(),
            Self::Flux(flux) => flux.identify(),
            Self::Authored(authored) => authored.identify(),
        }
    }

    pub(crate) fn is_modified(&self) -> bool {
        match self {
            Self::Space(space) => space.is_modified(),
            // Read-only, so there is never anything buffered to lose.
            Self::Archive(_) | Self::Flux(_) => false,
            Self::Authored(authored) => authored.is_modified(),
        }
    }

    /// That format's name, fit to show a user.
    pub(crate) fn format_name(&self) -> &'static str {
        match self {
            Self::Space(space) => space.descriptor().name,
            Self::Archive(archive) => archive.format_name(),
            Self::Flux(flux) => flux.format_name(),
            Self::Authored(authored) => authored.kind().name(),
        }
    }

    /// Every device type the recognizing format's adapter records
    /// (P12) — one where the format admits one, several where the
    /// caller declares which, and none for an archive grammar.
    pub(crate) fn recorded_devices(&self) -> &'static [DeviceType] {
        match self {
            Self::Space(space) => space.descriptor().devices,
            Self::Archive(_) | Self::Authored(_) => &[],
            Self::Flux(_) => &FLUX_RECORDED_DEVICES,
        }
    }

    /// The family of an artifact this release recognizes and holds in no
    /// device — flux, today.
    pub(crate) fn foreign_family(&self) -> Option<&'static str> {
        match self {
            Self::Space(space) => space.foreign_family(),
            // A flux medium loaded by its own declaration is not a
            // foreign family — it is the family, at home — and an
            // authored medium was read from no artifact at all.
            Self::Archive(_) | Self::Flux(_) | Self::Authored(_) => None,
        }
    }

    /// The space this medium presents, or the refusal naming the vantage
    /// it has instead.
    ///
    /// Every verb that addresses a space passes through here, so a
    /// namespace-native medium answers by name rather than by a verb
    /// failing further in — the same discipline an empty slot already
    /// answers a content verb with.
    pub(crate) fn space(&self, verb: &str) -> Result<&MediaState> {
        match self {
            Self::Space(space) => Ok(space),
            Self::Archive(archive) => Err(no_space(verb, archive)),
            Self::Flux(flux) => Err(flux_load::no_space(verb, flux)),
            Self::Authored(authored) => Err(authored.no_image(verb)),
        }
    }

    pub(crate) fn space_mut(&mut self, verb: &str) -> Result<&mut MediaState> {
        match self {
            Self::Space(space) => Ok(space),
            Self::Archive(archive) => Err(no_space(verb, archive)),
            Self::Flux(flux) => Err(flux_load::no_space(verb, flux)),
            Self::Authored(authored) => Err(authored.no_image(verb)),
        }
    }

    /// The archive this medium is, where it is one.
    pub(crate) fn archive(&self) -> Option<&ArchiveMedium> {
        match self {
            Self::Space(_) | Self::Flux(_) | Self::Authored(_) => None,
            Self::Archive(archive) => Some(archive),
        }
    }

    /// The flux state this medium homes, or the refusal naming the
    /// family it holds instead: the flux questions answer where the
    /// device type's profile bears flux, and nowhere else (P13, P30).
    pub(crate) fn flux_mut(&mut self, verb: &str) -> Result<&mut FluxState> {
        match self {
            Self::Flux(flux) => Ok(flux),
            Self::Space(space) => Err(Error::unsupported(format!(
                "'{verb}' reads a flux recording's presentation, and {} holds a \
                 block medium, whose recording is presented by its format \
                 adapter: the flux questions answer where the device type's \
                 profile bears flux (P13, P30)",
                space.named()
            ))),
            Self::Archive(archive) => Err(Error::unsupported(format!(
                "'{verb}' reads a flux recording's presentation, and {} holds an \
                 archive medium, which no device recorded: the flux questions \
                 answer where the device type's profile bears flux (P13, P30)",
                archive.named()
            ))),
            Self::Authored(authored) => Err(Error::unsupported(format!(
                "'{verb}' reads a flux recording's presentation, and {} was \
                 recorded by no device at all — the author created it whole: \
                 the flux questions answer where the device type's profile \
                 bears flux (P13, P30)",
                authored.named()
            ))),
        }
    }

    // ------------------------- the content an authored medium also bears

    /// The presented content's own size, for the media that present one.
    ///
    /// An authored medium answers here beside the block media: what its
    /// coordinates address *is* its content, and a blank article — which
    /// states no coordinates — refuses by name.
    pub(crate) fn presented_size(&self, verb: &str) -> Result<u64> {
        match self {
            Self::Space(space) => Ok(space.size()),
            Self::Authored(authored) => authored.space(verb).map(AuthoredSpace::size),
            Self::Archive(archive) => Err(no_space(verb, archive)),
            Self::Flux(flux) => Err(flux_load::no_space(verb, flux)),
        }
    }

    /// Reads within the presented content, the offset already resolved by
    /// whatever owns the bound.
    pub(crate) fn read_space_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<()> {
        match self {
            Self::Space(space) => space.read_space_at(offset, buf),
            Self::Authored(authored) => authored.space_mut("read_at")?.read_at(offset, buf),
            Self::Archive(archive) => Err(no_space("read_at", archive)),
            Self::Flux(flux) => Err(flux_load::no_space("read_at", flux)),
        }
    }

    /// Writes within the presented content, buffered until commit like
    /// every other write (P2).
    pub(crate) fn write_space_at(&mut self, offset: u64, data: &[u8]) -> Result<()> {
        match self {
            Self::Space(space) => space.write_space_at(offset, data),
            Self::Authored(authored) => authored.space_mut("write_at")?.write_at(offset, data),
            Self::Archive(archive) => Err(no_space("write_at", archive)),
            Self::Flux(flux) => Err(flux_load::no_space("write_at", flux)),
        }
    }

    /// The commit point (P2), wherever the medium's state lives: through
    /// to the artifact for a medium that has one, and into the session's
    /// own backing for a medium the author created.
    pub(crate) fn commit(&mut self) -> Result<()> {
        match self {
            Self::Space(space) => space.commit(),
            Self::Authored(authored) => authored.space_mut("commit")?.commit(),
            Self::Archive(archive) => Err(no_space("commit", archive)),
            Self::Flux(flux) => Err(flux_load::no_space("commit", flux)),
        }
    }

    /// Discards everything buffered since the medium was loaded or
    /// created, or since the last commit or rollback.
    pub(crate) fn rollback(&mut self) -> Result<()> {
        match self {
            Self::Space(space) => {
                space.rollback();
                Ok(())
            }
            Self::Authored(authored) => {
                authored.space_mut("rollback")?.rollback();
                Ok(())
            }
            Self::Archive(archive) => Err(no_space("rollback", archive)),
            Self::Flux(flux) => Err(flux_load::no_space("rollback", flux)),
        }
    }

    /// The partition pool this medium bears, established here and
    /// nowhere else — at the load, before the medium is handed to
    /// anyone (F56's "checked at load").
    ///
    /// **The pool populates under the device type's own spec, never by
    /// probing for a layout.** The hard-drive specs carry the scheme
    /// itself, so a medium recorded by one has that scheme's table
    /// checked against its content; the schemeless types — the floppy
    /// class, and the archive whose vantage is a namespace — bear the
    /// direct partition with no step at all, the table never read
    /// because no spec declared one.
    ///
    /// **Where the specified scheme does not check out, the answer is
    /// the direct partition rather than a refusal** (D32, kept): a
    /// hard-drive recording that carries no table is an unpartitioned
    /// disk, which is an ordinary disk this release reads, and refusing
    /// it would refuse every bare FAT image.
    pub(crate) fn establish_partitions(&mut self) -> Result<PartitionPool> {
        match self {
            Self::Archive(_) => Ok(PartitionPool::native_namespace()),
            Self::Flux(_) => Ok(PartitionPool::over_recording()),
            // An authored blank records no scheme — nobody recorded
            // anything on it — so it bears the direct partition over
            // whatever content its kind gave it, and nothing is read to
            // establish that: a blank the author just made is blank.
            Self::Authored(authored) => Ok(match authored.space("partitions") {
                Ok(space) => PartitionPool::authored_space(space.size()),
                Err(_) => PartitionPool::authored_blank(),
            }),
            Self::Space(space) => {
                let device = space.device_type().ok_or_else(|| {
                    Error::unsupported(format!(
                        "{} carries no device type, and a medium's content is \
                         laid out under the spec of the device that recorded \
                         it",
                        space.named()
                    ))
                })?;
                let length = space.size();
                match device.readable_scheme() {
                    Some(scheme) => {
                        let scheme = scheme?;
                        let discovery = space.check_scheme()?;
                        Ok(PartitionPool::over_space(scheme, &discovery, length))
                    }
                    None => {
                        let content = space.classify_content()?;
                        Ok(PartitionPool::over_schemeless(&content, length))
                    }
                }
            }
        }
    }

    /// The geometry this medium bears, established here and nowhere
    /// else — in the same act as the partition pool, before the medium
    /// is handed to anyone, and immutable from then on.
    ///
    /// **It is read, never declared.** Every source that speaks about
    /// the recording's coordinates is read once (P4), what they agree on
    /// is settled, and what they contradict each other about is reported
    /// as contradicted. Nothing here fails the load: a geometry is
    /// evidence about the artifact, so a source that cannot be read
    /// states nothing and the medium answers with whatever the others
    /// left.
    ///
    /// An archive has no coordinates at all — its content is reached by
    /// name — so it answers unstated without reading anything.
    pub(crate) fn establish_geometry(&mut self, partitions: &PartitionPool) -> Geometry {
        match self {
            // A flux recording's coordinates are the family's own
            // addressing, read through the presentation ladder; no
            // source below it states a cylinder-head-sector geometry.
            Self::Archive(_) | Self::Flux(_) => Geometry::unstated(),
            Self::Space(space) => space.establish_geometry(partitions),
            // Nothing is established for an authored medium, because
            // nothing is read: the author stated its coordinates when
            // they created it, and those are already its own.
            Self::Authored(authored) => authored.geometry(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::*;
    use super::*;
    use crate::io::device::AccessIntent;

    /// Exercises the whole public qcow2 path a caller runs: open,
    /// geometry, write, commit, reopen, read back.
    #[test]
    fn fat16_inside_qcow2_end_to_end() {
        let path =
            std::env::temp_dir().join(format!("remanence-qcow2-e2e-{}.qcow2", std::process::id()));
        let virtual_size = build_fat16_qcow2(&path);

        // Now the public path.
        let mut disk = MediaState::open(&path, AccessIntent::Write).expect("disk opens");
        assert!(matches!(disk.format(), DiskFormat::Qcow2 { version: 3 }));
        assert_eq!(disk.size(), virtual_size);

        let pool = pool_of(&mut disk);
        let report = disk.inspect(&pool).expect("inspection reads");
        assert_eq!(report.volumes.len(), 1);
        let composed = report.volumes[0].id;
        let volume = report.volumes[0].start_bytes;
        assert_eq!(
            report
                .filesystem_on(composed)
                .and_then(|fs| fs.label.as_ref())
                .and_then(|label| label.name.clone()),
            Some("REMANENCE".to_owned())
        );

        disk.make_directory(volume, "GUEST").expect("mkdir");
        disk.write_file(volume, "GUEST/PAYLOAD.BIN", b"through the mapping")
            .expect("write");
        assert_eq!(
            disk.stat(volume, "GUEST/PAYLOAD.BIN")
                .expect("stat")
                .map(|entry| entry.size_bytes),
            Some(b"through the mapping".len() as u64)
        );
        assert_eq!(disk.stat(volume, "GUEST/ABSENT.BIN").expect("stat"), None);
        disk.commit().expect("commit");
        drop(disk);

        let mut reopened = MediaState::open(&path, AccessIntent::Read).expect("reopens");
        assert_eq!(
            reopened
                .read_file(volume, "GUEST/PAYLOAD.BIN")
                .expect("read"),
            b"through the mapping"
        );
        drop(reopened);
        std::fs::remove_file(&path).ok();
    }

    /// Exercises the whole public VDI path a caller runs: open, geometry,
    /// write into a block the image never allocated, commit, reopen, read
    /// back.
    #[test]
    fn fat16_inside_vdi_end_to_end() {
        let path =
            std::env::temp_dir().join(format!("remanence-vdi-e2e-{}.vdi", std::process::id()));
        let virtual_size = build_fat16_vdi(&path);
        let before = std::fs::metadata(&path).expect("metadata").len();

        let mut disk = MediaState::open(&path, AccessIntent::Write).expect("disk opens");
        assert_eq!(disk.format(), DiskFormat::Vdi { major: 1, minor: 1 });
        assert_eq!(disk.size(), virtual_size);
        assert!(
            disk.image_size_bytes() < virtual_size,
            "a dynamic image is smaller than the disk it presents"
        );

        let pool = pool_of(&mut disk);
        let report = disk.inspect(&pool).expect("inspection reads");
        assert_eq!(report.volumes.len(), 1);
        let composed = report.volumes[0].id;
        let volume = report.volumes[0].start_bytes;
        assert_eq!(
            report
                .filesystem_on(composed)
                .and_then(|fs| fs.label.as_ref())
                .and_then(|label| label.name.clone()),
            Some("REMANENCE".to_owned())
        );

        // The file data lands in the volume's data area, which the
        // builder left unallocated: this write allocates.
        disk.make_directory(volume, "GUEST").expect("mkdir");
        disk.write_file(volume, "GUEST/PAYLOAD.BIN", &new_content())
            .expect("write");
        assert_eq!(
            std::fs::metadata(&path).expect("metadata").len(),
            before,
            "nothing reaches the file before the commit"
        );
        disk.commit().expect("commit");
        assert!(
            std::fs::metadata(&path).expect("metadata").len() > before,
            "the commit allocated new blocks into the image"
        );
        drop(disk);

        let mut reopened = MediaState::open(&path, AccessIntent::Read).expect("reopens");
        assert_eq!(
            reopened
                .read_file(volume, "GUEST/PAYLOAD.BIN")
                .expect("read"),
            new_content()
        );
        drop(reopened);
        std::fs::remove_file(&path).ok();
    }
}
