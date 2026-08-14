// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The machine-level report: what a machine's own disks say it is.
//!
//! A disk report answers for one medium. This answers for the **set** —
//! which devices the machine holds, what each of them turned out to be,
//! which of them the machine booted, and, where the booting volume names
//! an operating system whose letter rule this release claims, the letters
//! that system gave every volume in the machine.
//!
//! **Nothing here is asserted.** The caller states a machine — devices,
//! the order they attach, and the media in them — and every fact above
//! is read from the disks that machine holds. The one exception is a
//! declared boot device, which is configuration no artifact holds and is
//! marked as such wherever it appears.
//!
//! **A derived mapping is not evidence.** Letters are produced by a named
//! rule applied over what was read, and the rule and its inputs travel
//! with the answer as [`provenance`](MachineReport::provenance), never
//! as the observations recognition carries (P4).

use crate::error::Result;
use crate::filesystem::dos_install::{self, DosInstallation};
use crate::filesystem::dos_letters::{DosComposer, DriveMapping, LetterOutcome};
use crate::model::report::{DiskReport, VolumeId};

/// One device of the machine, and what the medium in it turned out to be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MachineDisk {
    /// The attachment identity this device answers to — `hdd0`, `fd0`.
    pub attachment: String,
    /// The device type's own name, or `None` for a slot that records
    /// nothing.
    pub device_type: Option<String>,
    /// The device class, in its stable cross-language spelling —
    /// `"floppy"` or `"hard-drive"` — which is what decides whether a
    /// claimed letter rule reaches this drive at all, and which of its
    /// two orders it belongs to.
    pub family: Option<String>,
    /// The inspection of the medium in it, or `None` where the device
    /// holds none.
    pub report: Option<DiskReport>,
    /// Why there is no report, where there is none. An empty device is
    /// configuration in its own right, not a failure.
    pub note: Option<String>,
}

/// One volume a machine holds, named the way its operating system named
/// it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MachineVolume {
    /// The device the volume sits on.
    pub attachment: String,
    /// The identity to pass back into a file verb. **This is the handle**
    /// — the letter beside it is what a user is shown.
    pub volume: VolumeId,
    /// The letter the booting system gave it, or `None` where the rules
    /// established none for it.
    pub letter: Option<char>,
}

/// What recognition established about a bootable installation found in
/// the machine, before anything settled which one booted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootCandidate {
    pub attachment: String,
    pub volume: VolumeId,
    /// The operating system found there, by its stable spelling.
    pub system: String,
    /// What recognized it (P4).
    pub evidence: Vec<String>,
}

/// Which volume the machine booted, and what settled it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BootOutcome {
    /// One installation booted, and this is it.
    Booted {
        attachment: String,
        installation: DosInstallation,
        /// `true` where the machine's own declaration settled it rather
        /// than the evidence — which is the caller saying the firmware
        /// booted something other than the default.
        declared: bool,
    },
    /// Several installations are bootable and nothing in the machine
    /// settles which one ran. Every candidate stands with its evidence,
    /// and no letters are established: choosing between them by size,
    /// order or which read cleanly would be a guess wearing an answer's
    /// clothes.
    Ambiguous { candidates: Vec<BootCandidate> },
    /// No volume in the machine holds an operating system this release
    /// recognizes.
    NothingBootable,
}

impl BootOutcome {
    /// The outcome's stable cross-language spelling.
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Booted { .. } => "booted",
            Self::Ambiguous { .. } => "ambiguous",
            Self::NothingBootable => "nothing-bootable",
        }
    }
}

/// The complete reading of one machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MachineReport {
    /// The machine's identity, or `None` for the session's anonymous
    /// machine.
    pub machine: Option<String>,
    /// Every device, in the order the machine attached them.
    pub disks: Vec<MachineDisk>,
    /// Which volume booted, and what settled it.
    pub boot: BootOutcome,
    /// Every letter the machine had a drive at, in letter order —
    /// undetermined ones included, a letter that exists and could not be
    /// settled being a different fact from one that does not exist.
    pub drives: Vec<DriveMapping>,
    /// The rule applied, what it was applied to, and what was read to
    /// choose it. **Not evidence** (P4).
    pub provenance: Vec<String>,
}

impl MachineReport {
    /// What this letter names, or `None` where the machine had no drive
    /// at it.
    pub fn letter(&self, letter: char) -> Option<&DriveMapping> {
        self.drives
            .iter()
            .find(|mapping| mapping.letter == letter.to_ascii_uppercase())
    }

    /// The volume this letter names, or `None` where the letter names no
    /// volume — an undetermined letter, a phantom drive, or a letter the
    /// machine has no drive at.
    pub fn volume_at(&self, letter: char) -> Option<VolumeId> {
        match &self.letter(letter)?.outcome {
            LetterOutcome::Volume { volume, .. } => Some(*volume),
            _ => None,
        }
    }

    /// Every volume the machine holds, each with the letter its operating
    /// system gave it where one was established.
    ///
    /// This is the volume-first view of the same answer the letters give:
    /// a volume with no letter is still a volume the machine holds, and
    /// it appears here rather than being dropped to keep a list tidy.
    pub fn volumes(&self) -> Vec<MachineVolume> {
        let mut volumes = Vec::new();
        for disk in &self.disks {
            let Some(report) = &disk.report else {
                continue;
            };
            for volume in &report.volumes {
                volumes.push(MachineVolume {
                    attachment: disk.attachment.clone(),
                    volume: volume.id,
                    letter: self.letter_of(volume.id),
                });
            }
        }
        volumes
    }

    /// The letter established for one volume, where the rules established
    /// one.
    pub fn letter_of(&self, volume: VolumeId) -> Option<char> {
        self.drives.iter().find_map(|mapping| match &mapping.outcome {
            LetterOutcome::Volume { volume: named, .. } if *named == volume => Some(mapping.letter),
            _ => None,
        })
    }
}

impl crate::model::machine::MachineView<'_> {
    /// Reads this machine: every device, what the medium in each turned
    /// out to be, which one it booted, and the drive letters its
    /// operating system gave.
    ///
    /// **The caller states the machine and nothing else.** Which system
    /// is installed, which volume boots, and what its startup
    /// configuration declared are all read from the disks the machine
    /// holds — they are recorded there, and this library is already
    /// reading those disks. The one fact no artifact holds is which
    /// device the firmware booted, and a caller who needs to override the
    /// evidence declares it with
    /// [`declare_boot_device`](crate::MachineView::declare_boot_device).
    ///
    /// A device holding no medium contributes no volume, and a family no
    /// claimed rule letters is passed over by name rather than silently:
    /// both appear in the report saying what they are.
    pub fn inspect(&mut self) -> Result<MachineReport> {
        let identity = self.identity().map(ToOwned::to_owned);
        let declared_boot = self.boot_device();

        let mut disks = Vec::new();
        let mut candidates: Vec<(String, DosInstallation)> = Vec::new();

        for attachment in self.attachments() {
            let device = self
                .device(attachment)
                .expect("an attachment this machine just listed");
            let device_type = device.device_type().map(|kind| kind.name().to_owned());
            let family = device.device_type().map(|kind| {
                match kind {
                    crate::DeviceType::Floppy(_) => "floppy",
                    crate::DeviceType::HardDrive(_) => "hard-drive",
                }
                .to_owned()
            });
            let slot = device.slot().name().to_owned();

            if !device.is_occupied() {
                disks.push(MachineDisk {
                    attachment: attachment.to_string(),
                    device_type,
                    family,
                    report: None,
                    note: Some(format!(
                        "the {slot} at {attachment} holds no medium, so it \
                         composed no volume"
                    )),
                });
                continue;
            }

            // A device recording no type is the archive receiver, whose
            // medium's vantage is a namespace rather than a space: it has
            // no scheme, no volume and no sector to address, so a disk
            // inspection is a category error rather than a failure, and
            // no assignment rule reaches it either.
            if family.is_none() {
                disks.push(MachineDisk {
                    attachment: attachment.to_string(),
                    device_type,
                    family,
                    report: None,
                    note: Some(format!(
                        "the {slot} at {attachment} bears a namespace rather than \
                         a disk, so it composes no volume and no claimed letter \
                         rule reaches it"
                    )),
                });
                continue;
            }

            let report = self
                .device_mut(attachment)
                .expect("a device this machine just listed")
                .medium_mut()
                .expect("a device this machine just found occupied")
                .inspect()?;

            // Recognition runs over each composed volume while the medium
            // is in hand, so nothing is opened twice.
            for volume in &report.volumes {
                let active = match &volume.origin {
                    crate::VolumeOrigin::Regions(regions) => regions.iter().any(|id| {
                        report.region(*id).is_some_and(|region| region.declared_active)
                    }),
                    crate::VolumeOrigin::WholeDevice => false,
                };
                let mut view = self
                    .device_mut(attachment)
                    .expect("a device this machine just listed");
                let medium = view
                    .medium_mut()
                    .expect("a device this machine just found occupied");
                if let Some(install) =
                    dos_install::recognize(medium, volume.id, volume.start_bytes, active)?
                {
                    candidates.push((attachment.to_string(), install));
                }
            }

            disks.push(MachineDisk {
                attachment: attachment.to_string(),
                device_type,
                family,
                report: Some(report),
                note: None,
            });
        }

        let boot = settle_boot(candidates, declared_boot.map(|id| id.to_string()));
        let mut provenance = Vec::new();
        let drives = compose_letters(&disks, &boot, &mut provenance)?;

        provenance.insert(
            0,
            match &identity {
                Some(identity) => format!(
                    "read from the machine identified '{identity}' — its device \
                     set in the order its devices were added, and the disks in it"
                ),
                None => "read from the session's anonymous machine — its device set \
                         in the order its devices were added, and the disks in it"
                    .to_owned(),
            },
        );

        Ok(MachineReport {
            machine: identity,
            disks,
            boot,
            drives,
            provenance,
        })
    }
}

/// Which candidate booted. A declaration governs where one was made; the
/// evidence settles it where exactly one candidate stands; and several
/// candidates with nothing between them stay several.
fn settle_boot(
    candidates: Vec<(String, DosInstallation)>,
    declared: Option<String>,
) -> BootOutcome {
    if let Some(attachment) = declared {
        let mut on_device: Vec<_> = candidates
            .iter()
            .filter(|(at, _)| *at == attachment)
            .collect();
        if on_device.len() == 1 {
            let (at, install) = on_device.remove(0);
            return BootOutcome::Booted {
                attachment: at.clone(),
                installation: install.clone(),
                declared: true,
            };
        }
        // A declaration names a device, not a volume. Where that device
        // holds several installations the declaration has not settled
        // which, and neither has anything else.
        if on_device.is_empty() {
            return BootOutcome::NothingBootable;
        }
        return BootOutcome::Ambiguous {
            candidates: on_device
                .into_iter()
                .map(|(at, install)| candidate(at, install))
                .collect(),
        };
    }

    if candidates.is_empty() {
        return BootOutcome::NothingBootable;
    }

    // The era's firmware order is a claimed rule like any other: a PC
    // boots the first attached fixed disk that can boot. So several
    // bootable disks do not make the machine unreadable — the first one
    // booted, and a caller whose firmware was set otherwise declares it.
    let first = candidates[0].0.clone();
    let on_first: Vec<_> = candidates.iter().filter(|(at, _)| *at == first).collect();

    // A tie *inside* one device is settled by the table's own boot flag,
    // which is what the boot sector follows: the active partition is the
    // one that loaded. Only where nothing is flagged does the tie stand.
    if on_first.len() > 1 {
        let mut flagged = on_first.iter().filter(|(_, install)| install.on_active_region);
        if let (Some((at, install)), None) = (flagged.next(), flagged.next()) {
            return BootOutcome::Booted {
                attachment: at.clone(),
                installation: (*install).clone(),
                declared: false,
            };
        }
        return BootOutcome::Ambiguous {
            candidates: on_first
                .into_iter()
                .map(|(at, install)| candidate(at, install))
                .collect(),
        };
    }

    let (attachment, installation) = candidates.into_iter().next().expect("at least one");
    BootOutcome::Booted {
        attachment,
        installation,
        declared: false,
    }
}

fn candidate(attachment: &str, install: &DosInstallation) -> BootCandidate {
    BootCandidate {
        attachment: attachment.to_owned(),
        volume: install.volume,
        system: install.kernel.name().to_owned(),
        evidence: install.evidence.clone(),
    }
}

/// The letters the booting installation's own rule assigns over the
/// machine's disks, or none where nothing established a rule to apply.
fn compose_letters(
    disks: &[MachineDisk],
    boot: &BootOutcome,
    provenance: &mut Vec<String>,
) -> Result<Vec<DriveMapping>> {
    let BootOutcome::Booted {
        installation,
        declared,
        attachment,
    } = boot
    else {
        provenance.push(match boot {
            BootOutcome::NothingBootable => "no volume in this machine holds an operating system \
                 this release recognizes, so no letter rule applies and none \
                 was assigned"
                .to_owned(),
            _ => "several volumes in this machine are bootable and nothing \
                  settles which one ran; every candidate stands with its \
                  evidence, and no letters were assigned"
                .to_owned(),
        });
        return Ok(Vec::new());
    };

    provenance.push(match declared {
        true => format!(
            "the machine declares it boots {attachment}, which governs over what \
             the disks make look bootable; that is configuration, not evidence"
        ),
        false => format!(
            "{attachment} is the first attached device holding a recognized \
             system, and the era's firmware order boots that one; declaring a \
             boot device overrides it"
        ),
    });
    provenance.extend(installation.evidence.iter().cloned());

    let rule = match installation.assignment_rule() {
        Ok(rule) => rule,
        Err(error) => {
            provenance.push(format!(
                "no letters were assigned: {}",
                error.to_string().to_lowercase()
            ));
            return Ok(Vec::new());
        }
    };

    // The composition itself is the claimed rule over the machine's own
    // disks, which is the same act the letter seam already performs and
    // is performed by it here rather than restated.
    let mut composer = DosComposer::new();
    let (mut floppy_slot, mut fixed_order) = (0u32, 0u32);
    for disk in disks {
        let Some(report) = &disk.report else {
            continue;
        };
        // Which of the two orders a drive belongs to is the device's own
        // family, not a caller's say-so. A family no claimed rule letters
        // is passed over here and said so below.
        match disk.family.as_deref() {
            Some("hard-drive") => {
                composer.add_fixed_disk(fixed_order, &disk.attachment, report)?;
                fixed_order += 1;
            }
            Some("floppy") if floppy_slot < 2 => {
                composer.add_floppy(floppy_slot, &disk.attachment, report)?;
                floppy_slot += 1;
            }
            other => provenance.push(format!(
                "{} was passed over: no claimed DOS rule letters {}",
                disk.attachment,
                other.unwrap_or("a device recording no type")
            )),
        }
    }
    for condition in &installation.conditions {
        composer.declare(*condition);
    }
    if let Some(letter) = installation.optical_letter {
        composer.place_optical(letter, "MSCDEX /L: in the machine's startup files")?;
    }

    let map = composer.compose(rule)?;
    provenance.extend(map.provenance);
    Ok(map.mappings)
}
