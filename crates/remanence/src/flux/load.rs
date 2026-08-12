// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The flux family's medium state: what a flux medium homes, and the
//! two declared loads that create one (F59).
//!
//! **A flux medium is the served form.** Its state is a
//! [`FluxMedium`] — what a drive of the family would be served, one
//! circular pulse stream per family-addressed location — and it arrives
//! two ways: a KryoFlux capture set reduces to it under the profile's
//! declared `Materialization` defaults, and a P64 container loads the
//! served form straight in. Either way the medium carries the whole
//! story as provenance: the member grammar's evidence, the profile
//! claim's verdict, the policy that ran, and the declared-loss account
//! (P29 — nothing unnamed).
//!
//! **The presentation ladder is materialized here, once, under the
//! profile's declarations** (P30 reached through the type): being a
//! `Commodore1541` medium *means* reading through the c1541 channel and
//! codec, so [`FluxState::bitstream`], [`FluxState::bytestream`] and
//! [`FluxState::sectors`] take no policy and answer the same state on
//! every call. The evidence stays behind the surface: no pulse, bit or
//! capture-run iterator leaves this module.

use std::fs::File;
use std::sync::Arc;

use crate::error::{Error, ErrorCategory, Result};
use crate::evidence::DeclaredLoss;
use crate::flux::c1541::presentation::{C1541Bitstream, C1541Bytestream, materialize_bitstream};
use crate::flux::c1541::sectors::C1541Sectors;
use crate::flux::drive_profile::DriveProfile;
use crate::flux::kryoflux::{self, MemberSource};
use crate::flux::medium::FluxMedium;
use crate::flux::remanence::reconstruction::{ReconstructionPolicy, RecordingSelection, plan};
use crate::io::device::{AccessMode, Claim, read_exact_at};
use crate::io::source::{FileSource, ImageSource};
use crate::model::assurance::Assurance;
use crate::model::device_type::{DeviceType, FloppyDrive};
use crate::model::media_profile::MediaProfile;
use crate::model::session::{
    Identification, Layer, LayerKind, LayerLayout, PhysicalMediaLayout, SizeInformation,
};

fn refuse(format_id: &str, reason: impl Into<String>) -> Error {
    Error::invalid_image(format_id, reason)
}

/// One member of a declared collection: the caller's own opened file,
/// with the name recovered from the handle — the member grammar's only
/// evidence of position.
struct HandleMember {
    name: String,
    file: Arc<File>,
    size: u64,
}

impl MemberSource for HandleMember {
    fn name(&self) -> &str {
        &self.name
    }

    fn size(&self) -> u64 {
        self.size
    }

    fn bytes(&self) -> Result<Vec<u8>> {
        let mut bytes = vec![0u8; self.size as usize];
        read_exact_at(&self.file, 0, &mut bytes)
            .map_err(|error| Error::io(format!("failed to read '{}': {error}", self.name)))?;
        Ok(bytes)
    }
}

/// One member gathered from another medium's namespace, riding the
/// claim of the medium it came from.
struct EntryMember(FileSource);

impl MemberSource for EntryMember {
    fn name(&self) -> &str {
        self.0.name()
    }

    fn size(&self) -> u64 {
        self.0.size()
    }

    fn bytes(&self) -> Result<Vec<u8>> {
        self.0.read_whole()
    }
}

/// The flux medium a session pools: the served form, its claims, its
/// account, and the presentation ladder above it.
#[derive(Debug)]
pub(crate) struct FluxState {
    /// The artifact claimed, where one name covers it — a P64's
    /// recovered name. A collection has no one path and answers none.
    path: Option<String>,
    device: FloppyDrive,
    profile: &'static DriveProfile,
    format_id: &'static str,
    format_name: &'static str,
    medium: FluxMedium,
    assurance: Assurance,
    /// The source's own bytes, summed — the raw plane's extent.
    source_bytes: u64,
    /// The P7 claims beneath the load, held for the medium's pooled
    /// life: the caller's own opened members, or the one handle the
    /// artifact (or the archive it came from) was claimed by.
    _claims: Vec<Arc<File>>,
    cache_bytes: u64,
    /// The ladder, materialized once on demand under the profile's
    /// declared policies and answered from then on.
    bitstream: Option<C1541Bitstream>,
    bytestream: Option<C1541Bytestream>,
    sectors: Option<C1541Sectors>,
}

impl FluxState {
    // ----------------------------------------------------- the two loads

    /// Loads a declared KryoFlux collection: member grammar,
    /// completeness, stream grammar and the profile claim checked
    /// whole, then the reduction under the profile's declared
    /// `Materialization` defaults (P29, P30).
    pub(crate) fn load_kryoflux(
        members: Vec<CollectionMember>,
        device: FloppyDrive,
        cache_bytes: u64,
    ) -> Result<Self> {
        let device_id = DeviceType::Floppy(device).id();
        let profile = device.flux_profile().ok_or_else(|| {
            // Unreachable through the format catalog, which enumerates
            // the pairings; spelled as the refusal it would be.
            Error::unsupported(format!(
                "a {device_id} claims no flux path, so no profile can check a \
                 capture against it"
            ))
        })?;

        let mut claims: Vec<Arc<File>> = Vec::new();
        let sources: Vec<Box<dyn MemberSource>> = members
            .into_iter()
            .map(|member| -> Result<Box<dyn MemberSource>> {
                match member {
                    CollectionMember::Handle(file) => {
                        let name = crate::io::handle::recovered_name(&file)
                            .and_then(|path| {
                                path.file_name()
                                    .map(|name| name.to_string_lossy().into_owned())
                            })
                            .ok_or_else(|| {
                                refuse(
                                    kryoflux::KRYOFLUX,
                                    "a KryoFlux member's position exists only in its \
                                     name, and one handed-over handle has no \
                                     recoverable name: the collection cannot be laid \
                                     out",
                                )
                            })?;
                        let size = file
                            .metadata()
                            .map_err(|error| {
                                Error::io(format!("cannot size the member '{name}': {error}"))
                            })?
                            .len();
                        let file = Arc::new(file);
                        claims.push(Arc::clone(&file));
                        Ok(Box::new(HandleMember { name, file, size }))
                    }
                    CollectionMember::Entry(entry) => {
                        claims.push(entry.claim_handle());
                        Ok(Box::new(EntryMember(entry)))
                    }
                }
            })
            .collect::<Result<_>>()?;
        claims.sort_by(|left, right| Arc::as_ptr(left).cmp(&Arc::as_ptr(right)));
        claims.dedup_by(|left, right| Arc::ptr_eq(left, right));

        let assembled = kryoflux::assemble("the declared collection", &sources, cache_bytes)?;
        let mut evidence = assembled.evidence.clone();

        // The profile claim, checked whole: the declaration names the
        // device, the device's flux path names the profile, and the
        // capture must actually bear that profile's claim — recognition
        // is pinned to the declared profile and never shopped around.
        let recognition =
            crate::flux::drive_profile::recognition(&assembled.capture, Some(profile.id))?;
        let verdict = recognition
            .verdicts
            .first()
            .expect("a pinned recognition answers with the pinned profile's verdict");
        let mut sides: Vec<u64> = verdict
            .locations
            .iter()
            .filter(|location| location.claimed)
            .filter_map(|location| location.head)
            .collect();
        sides.sort_unstable();
        sides.dedup();
        let side = match sides.as_slice() {
            [] => {
                return Err(refuse(
                    kryoflux::KRYOFLUX,
                    format!(
                        "the declared collection does not bear the '{}' claim: the \
                         profile claimed none of the capture's {} locations \
                         (confidence {})",
                        profile.id,
                        verdict.locations.len(),
                        verdict.confidence
                    ),
                ));
            }
            [side] => *side,
            several => {
                return Err(refuse(
                    kryoflux::KRYOFLUX,
                    format!(
                        "the '{}' profile claims recordings on capture heads {} \
                         where the family records one surface; which head carries \
                         the disk is measured, and this capture measures as more \
                         than one",
                        profile.id,
                        several
                            .iter()
                            .map(u64::to_string)
                            .collect::<Vec<_>>()
                            .join(" and ")
                    ),
                ));
            }
        };
        evidence.push(format!(
            "the '{}' claim checked whole and borne: {} of {} declared locations \
             claimed at confidence {}, the recording measured on capture head \
             {side} — the other head bearing no recording, which is how a \
             single-surface family's capture reads",
            profile.id, verdict.locations_claimed, verdict.locations_declared, verdict.confidence
        ));

        // The reduction, under the profile's declared defaults: the
        // recordings measured by the count-spread discriminator, and
        // everything else the Materialization declaration states.
        let policy = ReconstructionPolicy {
            side,
            recordings: RecordingSelection::Measured,
        };
        let reduction = plan(&assembled.capture, &policy)?;
        let report = reduction.report();
        evidence.push(format!(
            "reduced under the '{}' profile's declared materialization defaults \
             (P30 reached through the type): side {side}, recordings measured by \
             the count-spread discriminator, {} of {} swept positions resolving \
             recordings",
            profile.id,
            report.recorded_positions.len(),
            report.swept_positions
        ));
        push_loss(&mut evidence, "the reduction", &report.declared_loss);
        for line in &report.evidence {
            evidence.push(format!("the reduction: {line}"));
        }
        let image = reduction.execute(cache_bytes)?;

        // The served projection: the reduced image is physical stratum,
        // and the medium a drive is served stands on the projection of
        // it at the family's own reference frame.
        let (medium, projection) = image.served_medium()?;
        push_loss(
            &mut evidence,
            "the served projection",
            &projection.into_entries(),
        );

        let mut assurance = Assurance::verified(
            assembled.source_bytes,
            AccessMode::ReadOnly,
            Claim::CallerOpened,
        );
        assurance.evidence = evidence;

        Ok(Self {
            path: None,
            device,
            profile,
            format_id: "kryoflux",
            format_name: "KryoFlux capture set",
            medium,
            assurance,
            source_bytes: assembled.source_bytes,
            _claims: claims,
            cache_bytes,
            bitstream: None,
            bytestream: None,
            sectors: None,
        })
    }

    /// Loads a P64 container's served form straight in (D31): the one
    /// format whose artifact already holds a flux medium at rest.
    pub(crate) fn load_p64(
        source: &ImageSource,
        path: Option<String>,
        claim: Claim,
        cache_bytes: u64,
    ) -> Result<Self> {
        let named = crate::model::media::named(path.as_deref());
        let (medium, report) = crate::flux::p64::decode(source, &named, cache_bytes)?;

        let mut evidence = report.evidence.clone();
        push_loss(&mut evidence, "the container", &report.declared_loss);

        let mut assurance = Assurance::verified(source.len(), AccessMode::ReadOnly, claim);
        assurance.evidence = evidence;

        Ok(Self {
            path,
            device: FloppyDrive::Commodore1541,
            profile: &crate::flux::drive_profile::C1541,
            format_id: "p64",
            format_name: "P64 flux image",
            source_bytes: source.len(),
            _claims: vec![source.claim_handle()],
            medium,
            assurance,
            cache_bytes,
            bitstream: None,
            bytestream: None,
            sectors: None,
        })
    }

    // ------------------------------------------------------- plain facts

    pub(crate) fn path(&self) -> Option<&str> {
        self.path.as_deref()
    }

    pub(crate) fn named(&self) -> String {
        match &self.path {
            Some(path) => format!("'{path}'"),
            None => "this flux medium (loaded from a declared collection)".to_owned(),
        }
    }

    pub(crate) fn device_type(&self) -> DeviceType {
        DeviceType::Floppy(self.device)
    }

    pub(crate) fn media(&self) -> &'static MediaProfile {
        self.profile.media
    }

    pub(crate) fn mode(&self) -> AccessMode {
        self.assurance.access
    }

    pub(crate) fn assurance(&self) -> &Assurance {
        &self.assurance
    }

    pub(crate) fn format_id(&self) -> &'static str {
        self.format_id
    }

    pub(crate) fn format_name(&self) -> &'static str {
        self.format_name
    }

    pub(crate) fn source_bytes(&self) -> u64 {
        self.source_bytes
    }

    /// The layered identification: the container or collection format,
    /// and the article the family's profile declares it is served on.
    pub(crate) fn identify(&self) -> Identification {
        Identification {
            layers: vec![
                Layer {
                    kind: LayerKind::Image,
                    id: self.format_id.to_owned(),
                    name: self.format_name.to_owned(),
                    confidence: 100,
                    known: true,
                    size: SizeInformation {
                        current_bytes: Some(self.source_bytes),
                        expected_bytes: None,
                    },
                    layout: LayerLayout::Unknown,
                },
                Layer {
                    kind: LayerKind::PhysicalMedia,
                    id: self.profile.media.id.to_owned(),
                    name: self.profile.media.name.to_owned(),
                    confidence: 100,
                    known: true,
                    size: SizeInformation::unknown(),
                    layout: LayerLayout::PhysicalMedia(PhysicalMediaLayout::Unknown),
                },
            ],
            modified: false,
            evidence: self.assurance.evidence.clone(),
        }
    }

    // ------------------------------------------------------- the ladder

    /// The family's hardware bitstream, materialized once under the
    /// profile's declared channel policy (P30 reached through the type).
    pub(crate) fn bitstream(&mut self) -> Result<&C1541Bitstream> {
        if self.bitstream.is_none() {
            self.bitstream = Some(materialize_bitstream(
                &self.medium,
                self.profile.presentation.channel_policy,
                self.cache_bytes,
            )?);
        }
        Ok(self.bitstream.as_ref().expect("just materialized"))
    }

    /// The family's encoded bytestream, materialized once above the
    /// bitstream under the profile's declared codec policy.
    pub(crate) fn bytestream(&mut self) -> Result<&C1541Bytestream> {
        if self.bytestream.is_none() {
            let cache_bytes = self.cache_bytes;
            let bitstream = self.bitstream()?;
            let bytestream = bitstream.materialize_c1541_bytestream(cache_bytes)?;
            self.bytestream = Some(bytestream);
        }
        Ok(self.bytestream.as_ref().expect("just materialized"))
    }

    /// The recording's own records, recognized once above the
    /// bytestream under the profile's declared record grammar and
    /// sector reading — the layer the CBM DOS namespace opens over.
    pub(crate) fn sectors(&mut self) -> Result<&C1541Sectors> {
        if self.sectors.is_none() {
            let cache_bytes = self.cache_bytes;
            let bytestream = self.bytestream()?;
            let sectors = bytestream.recognize_c1541_sectors(cache_bytes)?;
            self.sectors = Some(sectors);
        }
        Ok(self.sectors.as_ref().expect("just recognized"))
    }
}

/// One member of a declared collection, as the load receives it.
pub(crate) enum CollectionMember {
    /// The caller's own opened file (P7 as amended: their open, their
    /// lock, the claim's class recorded on the medium).
    Handle(File),
    /// A file gathered from another medium's namespace, riding that
    /// medium's claim.
    Entry(FileSource),
}

/// States a declared-loss account into the evidence, entry by entry — a
/// count is not an account, so each line says what it was.
fn push_loss(evidence: &mut Vec<String>, owner: &str, loss: &[DeclaredLoss]) {
    for entry in loss {
        evidence.push(format!(
            "{owner} declares loss '{}' × {}: {}",
            entry.code, entry.count, entry.detail
        ));
    }
}

/// The refusal a flux medium answers a byte-plane or space verb with.
pub(crate) fn no_space(verb: &str, state: &FluxState) -> Error {
    Error::categorized_image(
        ErrorCategory::Unsupported,
        state.format_id(),
        format!(
            "'{verb}' addresses a byte plane and {} holds a flux medium, whose \
             recording is pulses in an exact rotational frame (P13): its content \
             is reached through the presentation ladder — bitstream(), \
             bytestream(), and the namespace door of the direct partition it \
             bears",
            state.named()
        ),
    )
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::error::Result;
    use crate::evidence::Provenance;
    use crate::flux::drive_profile::C1541;
    use crate::flux::medium::{
        Derivation, FluxMedium, LocationKey, MediumBuilder, Pulse, Strength,
    };
    use crate::model::device_type::DeviceType;
    use crate::model::machine::Session;
    use crate::model::media::Format;

    /// A small medium of the family, written out as a P64 artifact —
    /// the served form at rest, which is what `Format::P64` loads
    /// straight in.
    fn p64_artifact(tag: &str) -> PathBuf {
        fn medium_of(pulses: &[(u64, u32)]) -> FluxMedium {
            let frame = crate::flux::p64::container_frame(
                Provenance::new("p64").note("declared for the test"),
            )
            .expect("the container and the profile declare the same frame");
            let mut builder = MediumBuilder::new(
                C1541.id,
                C1541.media,
                frame,
                Derivation::SelectedAndProjected,
                Provenance::new(C1541.id).note("mastered for the test"),
                Vec::new(),
            )
            .expect("the policy is stated");
            let built: Vec<Pulse> = pulses
                .iter()
                .map(|(position, state)| Pulse::new(*position, Strength::certain(*state)))
                .collect();
            builder
                .add_location(
                    LocationKey::new(C1541.id, 18, 0),
                    &built,
                    &[],
                    Provenance::new(C1541.id).note("reduced for the test"),
                )
                .expect("the location");
            let (mut medium, bytes, total) = builder.seal().expect("the backing seals");
            struct Bytes(Vec<u8>);
            impl crate::flux::capture::ByteSource for Bytes {
                fn read_at(&self, offset: u64, into: &mut [u8]) -> Result<()> {
                    let at = offset as usize;
                    into.copy_from_slice(&self.0[at..at + into.len()]);
                    Ok(())
                }
            }
            medium.attach_backing(Box::new(Bytes(bytes)), total, 1 << 20);
            medium
        }
        let medium = medium_of(&[(0, 2), (64, 2), (65, 1), (2_000_000, 0), (3_199_999, 2)]);
        let path = std::env::temp_dir().join(format!(
            "remanence-flux-media-{tag}-{}.p64",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        crate::flux::p64::write_new_artifact(&medium, &path).expect("the artifact writes");
        path
    }

    #[test]
    fn a_p64_loads_the_served_form_straight_in() {
        let path = p64_artifact("load");
        let mut session = Session::new();

        // The declared load, through the same verb every other medium
        // arrives through (D31): the artifact already holds a flux
        // medium at rest.
        let disk = session
            .load_media(
                std::fs::File::open(&path).expect("the artifact opens"),
                Format::P64,
            )
            .expect("the served form loads straight in");
        assert_eq!(
            disk.device_type(),
            Some(DeviceType::Floppy(
                crate::model::device_type::FloppyDrive::Commodore1541
            ))
        );
        assert_eq!(disk.article(), "flexible-5.25-soft");
        assert!(disk.path().is_some(), "the handle's name is recovered");

        // The container's account rides the medium as provenance,
        // declared loss included.
        let assurance = disk.assurance();
        assert!(
            assurance
                .evidence
                .iter()
                .any(|line| line.contains("opens with the P64 signature")),
            "{:?}",
            assurance.evidence
        );
        assert!(
            assurance
                .evidence
                .iter()
                .any(|line| line.contains("declares loss 'container-provenance'")),
            "{:?}",
            assurance.evidence
        );

        // The direct partition, and the argument-free rungs: the type
        // carries the channel and the codec.
        assert_eq!(disk.partition_scheme(), None);
        assert!(disk.partitions()[0].is_direct());
        let report = disk.bitstream().expect("the channel clocks it").inspect();
        assert_eq!(report.profile_id, "c1541");
        assert_eq!(report.locations.len(), 1);
        let bytestream = disk.bytestream().expect("the codec resolves it");
        assert_eq!(bytestream.inspect().codec_id, "c1541-gcr");

        let disk_id = disk.id();
        session.release_media(disk_id).expect("released");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn a_collection_offered_to_the_one_artifact_format_is_refused_by_name() {
        let path = p64_artifact("shape");
        let mut session = Session::new();
        let error = session
            .load_media(
                vec![std::fs::File::open(&path).expect("the artifact opens")],
                Format::P64,
            )
            .expect_err("a P64 is one artifact");
        assert!(error.to_string().contains("one artifact"), "{error}");
        std::fs::remove_file(&path).ok();
    }
}
