// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! `remanence_medium_inspect` and the report it answers: the device, the
//! partition schema, the regions, the volumes and the filesystems, each
//! with the evidence behind it.

use crate::abi::{RemanenceErrorCategory, clear_error, set_error, to_cstring};
use crate::session::RemanenceMedium;
use remanence::{DiskContent, RegionRole, VolumeOrigin};
use std::ffi::{CString, c_char};
use std::ptr;

// ---------------------------------------------------------------------------
// The layered disk inspection report: one owned handle over the whole
// report graph, with indexed bounds-checked access to its records and
// relationships. Strings are borrowed from the handle that owns them, and
// identities cross the ABI as opaque values a caller round-trips without
// parsing.
//
// **It is a view derived from the partition pool, and nothing navigates
// through it.** Content is reached through `remanence_medium_partition`;
// the report is what a caller shows a user, and the volume identities in
// it are what a caller carries back into a verb. The direct partition is a composition act
// rather than something a scheme declared, so it never appears as a
// region here: a medium recording no scheme reports zero regions.

/// A snapshot of one disk's layered inspection. Owned by the caller and
/// released with `remanence_report_free`; every string and record
/// reached through it is borrowed from it and dies with it.
pub struct RemanenceDiskReport {
    device_id: u64,
    device_image_format: CString,
    device_length_bytes: u64,
    device_article: CString,
    device_device_type: Option<CString>,
    device_authoritative_layer: CString,
    device_active_layer: CString,
    content: RemanenceDiskContent,
    content_evidence: Option<CString>,
    schema: Option<SchemaView>,
    regions: Vec<RegionView>,
    volumes: Vec<VolumeRecordView>,
    filesystems: Vec<FilesystemView>,
}

/// What the device's leading structure turned out to be. The report states
/// this rather than leaving a caller to reconstruct it from empty lists.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RemanenceDiskContent {
    /// All zero: a blank disk, which is an answer.
    Blank,
    /// A partition schema was recognized, whether or not a volume composed.
    Schema,
    /// No schema, and the whole device is one volume.
    DirectVolume,
    /// Not blank, and no adapter claims it. An outcome, not a refusal.
    UnknownNonblank,
}

/// How a schema declares a region: data, which composition may consume, or
/// structure, which it may not.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RemanenceRegionRole {
    Data,
    Structure,
}

/// Where a volume's storage came from.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RemanenceVolumeOrigin {
    WholeDevice,
    Regions,
}

pub(crate) struct SchemaView {
    kind: CString,
    evidence: Vec<CString>,
}

pub(crate) struct IssueView {
    pub(crate) category: RemanenceErrorCategory,
    pub(crate) message: CString,
}

pub(crate) struct RegionView {
    id: u64,
    declared_number: u32,
    declared_placement: CString,
    role: RemanenceRegionRole,
    declared_type: u8,
    declared_type_reading: CString,
    claimed: bool,
    start_bytes: u64,
    length_bytes: u64,
    issue: Option<IssueView>,
}

pub(crate) struct VolumeRecordView {
    id: u64,
    origin: RemanenceVolumeOrigin,
    origin_regions: Vec<u64>,
    start_bytes: u64,
    length_bytes: u64,
    evidence: Vec<CString>,
}

/// One source's reading of a volume label. `stored` is null where the
/// format gives this volume no such field at all, which is distinct from
/// a field that is present and blank.
pub(crate) struct LabelReadingView {
    source: CString,
    stored: Option<CString>,
}

/// A recognized volume's label answer, with every reading beside it.
pub(crate) struct VolumeLabelView {
    name: Option<CString>,
    answered_by: Option<CString>,
    readings: Vec<LabelReadingView>,
}

pub(crate) struct FilesystemView {
    id: u64,
    volume: u64,
    kind: Option<CString>,
    label: Option<VolumeLabelView>,
    cluster_bytes: Option<u64>,
    cluster_count: Option<u64>,
    sectors_per_track: Option<u16>,
    heads: Option<u16>,
    cylinders: Option<u64>,
    issues: Vec<IssueView>,
}

pub(crate) fn issue_view(issue: &remanence::Error) -> IssueView {
    IssueView {
        category: issue.category().into(),
        message: to_cstring(&issue.to_string()),
    }
}

pub(crate) fn evidence_views(evidence: &[String]) -> Vec<CString> {
    evidence.iter().map(|line| to_cstring(line)).collect()
}

pub(crate) fn label_view(label: &remanence::VolumeLabel) -> VolumeLabelView {
    VolumeLabelView {
        name: label.name.as_deref().map(to_cstring),
        answered_by: label.answered_by.as_deref().map(to_cstring),
        readings: label
            .readings
            .iter()
            .map(|reading| LabelReadingView {
                source: to_cstring(&reading.source),
                stored: reading.stored.as_deref().map(to_cstring),
            })
            .collect(),
    }
}

/// Inspects the medium and returns its layered report, derived from the
/// pool the load established. Null on failure, with the category and
/// message written to the out-parameters.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_medium_inspect(
    medium: *mut RemanenceMedium,
    error_category_out: *mut RemanenceErrorCategory,
    error_out: *mut *mut c_char,
    error_rule_out: *mut *mut c_char,
) -> *mut RemanenceDiskReport {
    unsafe { clear_error(error_out, error_rule_out) };
    let Some(handle) = (unsafe { medium.as_mut() }) else {
        return ptr::null_mut();
    };
    let Some(medium) = handle.medium() else {
        let error = remanence::Error::io("this medium was released");
        unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
        return ptr::null_mut();
    };
    match medium.inspect() {
        Ok(report) => {
            let (content, content_evidence) = match &report.content {
                DiskContent::Blank => (RemanenceDiskContent::Blank, None),
                DiskContent::Schema => (RemanenceDiskContent::Schema, None),
                DiskContent::DirectVolume => (RemanenceDiskContent::DirectVolume, None),
                DiskContent::UnknownNonblank { evidence } => (
                    RemanenceDiskContent::UnknownNonblank,
                    Some(to_cstring(evidence)),
                ),
            };
            let schema = report.partition_schema.as_ref().map(|schema| SchemaView {
                kind: to_cstring(&schema.kind),
                evidence: evidence_views(&schema.evidence),
            });
            let regions = report
                .regions
                .iter()
                .map(|region| RegionView {
                    id: region.id.value(),
                    declared_number: region.declared_number,
                    declared_placement: to_cstring(&region.declared_placement),
                    role: match region.role {
                        RegionRole::Data => RemanenceRegionRole::Data,
                        RegionRole::Structure => RemanenceRegionRole::Structure,
                    },
                    declared_type: region.declared_type,
                    declared_type_reading: to_cstring(&region.declared_type_reading),
                    claimed: region.claimed,
                    start_bytes: region.start_bytes,
                    length_bytes: region.length_bytes,
                    issue: region.issue.as_ref().map(issue_view),
                })
                .collect();
            let volumes = report
                .volumes
                .iter()
                .map(|volume| VolumeRecordView {
                    id: volume.id.value(),
                    origin: match &volume.origin {
                        VolumeOrigin::WholeDevice => RemanenceVolumeOrigin::WholeDevice,
                        VolumeOrigin::Regions(_) => RemanenceVolumeOrigin::Regions,
                    },
                    origin_regions: match &volume.origin {
                        VolumeOrigin::WholeDevice => Vec::new(),
                        VolumeOrigin::Regions(regions) => {
                            regions.iter().map(|region| region.value()).collect()
                        }
                    },
                    start_bytes: volume.start_bytes,
                    length_bytes: volume.length_bytes,
                    evidence: evidence_views(&volume.evidence),
                })
                .collect();
            let filesystems = report
                .filesystems
                .iter()
                .map(|filesystem| FilesystemView {
                    id: filesystem.id.value(),
                    volume: filesystem.volume.value(),
                    kind: filesystem.kind.as_deref().map(to_cstring),
                    label: filesystem.label.as_ref().map(label_view),
                    cluster_bytes: filesystem.cluster_bytes,
                    cluster_count: filesystem.cluster_count,
                    sectors_per_track: filesystem.declared_geometry.sectors_per_track,
                    heads: filesystem.declared_geometry.heads,
                    cylinders: filesystem.declared_geometry.cylinders,
                    issues: filesystem.issues.iter().map(issue_view).collect(),
                })
                .collect();
            Box::into_raw(Box::new(RemanenceDiskReport {
                device_id: report.device.id,
                device_image_format: to_cstring(&report.device.image_format),
                device_length_bytes: report.device.length_bytes,
                device_article: to_cstring(&report.device.article),
                device_device_type: report.device.device_type.as_deref().map(to_cstring),
                device_authoritative_layer: to_cstring(&report.device.authoritative_layer),
                device_active_layer: to_cstring(&report.device.active_layer),
                content,
                content_evidence,
                schema,
                regions,
                volumes,
                filesystems,
            }))
        }
        Err(error) => {
            unsafe { set_error(error_category_out, error_out, error_rule_out, &error) };
            ptr::null_mut()
        }
    }
}

/// Frees an inspection report and everything borrowed from it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_free(report: *mut RemanenceDiskReport) {
    if !report.is_null() {
        drop(unsafe { Box::from_raw(report) });
    }
}

pub(crate) unsafe fn region_view<'a>(
    report: *const RemanenceDiskReport,
    index: usize,
) -> Option<&'a RegionView> {
    unsafe { report.as_ref() }?.regions.get(index)
}

pub(crate) unsafe fn volume_record_view<'a>(
    report: *const RemanenceDiskReport,
    index: usize,
) -> Option<&'a VolumeRecordView> {
    unsafe { report.as_ref() }?.volumes.get(index)
}

pub(crate) unsafe fn filesystem_view<'a>(
    report: *const RemanenceDiskReport,
    index: usize,
) -> Option<&'a FilesystemView> {
    unsafe { report.as_ref() }?.filesystems.get(index)
}

/// The device identity assigned by this loaded composition (P21), scoped
/// to the open.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_device_id(report: *const RemanenceDiskReport) -> u64 {
    unsafe { report.as_ref() }.map_or(0, |report| report.device_id)
}

/// The image format the artifact turned out to be.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_device_image_format(
    report: *const RemanenceDiskReport,
) -> *const c_char {
    unsafe { report.as_ref() }.map_or(ptr::null(), |report| report.device_image_format.as_ptr())
}

/// The device's addressable length in bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_device_length_bytes(
    report: *const RemanenceDiskReport,
) -> u64 {
    unsafe { report.as_ref() }.map_or(0, |report| report.device_length_bytes)
}

/// The article of the medium attached to the device (P14) — the
/// substrate, said in the article catalog's own name for it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_device_article(
    report: *const RemanenceDiskReport,
) -> *const c_char {
    unsafe { report.as_ref() }.map_or(ptr::null(), |report| report.device_article.as_ptr())
}

/// The device the medium's content was recorded by, by the device
/// catalog's stable spelling — null where no device recorded it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_device_type(
    report: *const RemanenceDiskReport,
) -> *const c_char {
    unsafe { report.as_ref() }
        .and_then(|report| report.device_device_type.as_ref())
        .map_or(ptr::null(), |device| device.as_ptr())
}

/// The layer the image is authoritative at (P13).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_device_authoritative_layer(
    report: *const RemanenceDiskReport,
) -> *const c_char {
    unsafe { report.as_ref() }.map_or(ptr::null(), |report| {
        report.device_authoritative_layer.as_ptr()
    })
}

/// The layer active for this composition (P23).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_device_active_layer(
    report: *const RemanenceDiskReport,
) -> *const c_char {
    unsafe { report.as_ref() }.map_or(ptr::null(), |report| report.device_active_layer.as_ptr())
}

/// What the device's leading structure turned out to be.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_content(
    report: *const RemanenceDiskReport,
) -> RemanenceDiskContent {
    unsafe { report.as_ref() }.map_or(RemanenceDiskContent::Blank, |report| report.content)
}

/// Why no adapter claimed the content, for the unknown-nonblank outcome;
/// null for every other outcome.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_content_evidence(
    report: *const RemanenceDiskReport,
) -> *const c_char {
    unsafe { report.as_ref() }.map_or(ptr::null(), |report| {
        report
            .content_evidence
            .as_ref()
            .map_or(ptr::null(), |evidence| evidence.as_ptr())
    })
}

/// Whether a partition schema was recognized.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_has_partition_schema(
    report: *const RemanenceDiskReport,
) -> bool {
    unsafe { report.as_ref() }.is_some_and(|report| report.schema.is_some())
}

/// The recognized schema's kind, or null where none was recognized.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_partition_schema_kind(
    report: *const RemanenceDiskReport,
) -> *const c_char {
    unsafe { report.as_ref() }.map_or(ptr::null(), |report| {
        report
            .schema
            .as_ref()
            .map_or(ptr::null(), |schema| schema.kind.as_ptr())
    })
}

/// How many evidence lines the schema recognition carries.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_partition_schema_evidence_count(
    report: *const RemanenceDiskReport,
) -> usize {
    unsafe { report.as_ref() }.map_or(0, |report| {
        report
            .schema
            .as_ref()
            .map_or(0, |schema| schema.evidence.len())
    })
}

/// One evidence line from the schema recognition.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_partition_schema_evidence(
    report: *const RemanenceDiskReport,
    index: usize,
) -> *const c_char {
    unsafe { report.as_ref() }.map_or(ptr::null(), |report| {
        report.schema.as_ref().map_or(ptr::null(), |schema| {
            schema
                .evidence
                .get(index)
                .map_or(ptr::null(), |line| line.as_ptr())
        })
    })
}

/// How many regions the schema declares. Every declared region is
/// reported, refused ones included.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_region_count(
    report: *const RemanenceDiskReport,
) -> usize {
    unsafe { report.as_ref() }.map_or(0, |report| report.regions.len())
}

/// A region's opaque identity. Pass it back to the library; never parse
/// it, and never build one.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_region_id(
    report: *const RemanenceDiskReport,
    index: usize,
) -> u64 {
    unsafe { region_view(report, index) }.map_or(0, |region| region.id)
}

/// The number the schema itself declared this region at.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_region_declared_number(
    report: *const RemanenceDiskReport,
    index: usize,
) -> u32 {
    unsafe { region_view(report, index) }.map_or(0, |region| region.declared_number)
}

/// How the schema places this region in its own vocabulary: for MBR,
/// "primary" for one of the four slots and "logical" for an entry on the
/// extended chain. A different axis from the role: the extended partition
/// is a primary slot whose role is structural.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_region_declared_placement(
    report: *const RemanenceDiskReport,
    index: usize,
) -> *const c_char {
    unsafe { region_view(report, index) }
        .map_or(ptr::null(), |region| region.declared_placement.as_ptr())
}

/// Whether the schema declares this region as data or as structure.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_region_role(
    report: *const RemanenceDiskReport,
    index: usize,
) -> RemanenceRegionRole {
    unsafe { region_view(report, index) }.map_or(RemanenceRegionRole::Data, |region| region.role)
}

/// The type value exactly as the schema records it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_region_declared_type(
    report: *const RemanenceDiskReport,
    index: usize,
) -> u8 {
    unsafe { region_view(report, index) }.map_or(0, |region| region.declared_type)
}

/// What that value declares, in a sentence fit to quote in a refusal.
/// Present whether or not this release reads the type, and it describes
/// the declaration rather than the content.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_region_declared_type_reading(
    report: *const RemanenceDiskReport,
    index: usize,
) -> *const c_char {
    unsafe { region_view(report, index) }
        .map_or(ptr::null(), |region| region.declared_type_reading.as_ptr())
}

/// Whether this release reads the declared type.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_region_is_claimed(
    report: *const RemanenceDiskReport,
    index: usize,
) -> bool {
    unsafe { region_view(report, index) }.is_some_and(|region| region.claimed)
}

/// Where the region starts, in bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_region_start_bytes(
    report: *const RemanenceDiskReport,
    index: usize,
) -> u64 {
    unsafe { region_view(report, index) }.map_or(0, |region| region.start_bytes)
}

/// How long the region is, in bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_region_length_bytes(
    report: *const RemanenceDiskReport,
    index: usize,
) -> u64 {
    unsafe { region_view(report, index) }.map_or(0, |region| region.length_bytes)
}

/// The region's refusal category; false where the region reads cleanly.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_region_issue_category(
    report: *const RemanenceDiskReport,
    index: usize,
    category_out: *mut RemanenceErrorCategory,
) -> bool {
    let Some(issue) = (unsafe { region_view(report, index) }).and_then(|r| r.issue.as_ref()) else {
        return false;
    };
    if let Some(out) = unsafe { category_out.as_mut() } {
        *out = issue.category;
    }
    true
}

/// The region's refusal, or null where the region reads cleanly.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_region_issue(
    report: *const RemanenceDiskReport,
    index: usize,
) -> *const c_char {
    unsafe { region_view(report, index) }.map_or(ptr::null(), |region| {
        region
            .issue
            .as_ref()
            .map_or(ptr::null(), |issue| issue.message.as_ptr())
    })
}

/// How many volumes were composed, whatever was recognized on them.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_volume_count(
    report: *const RemanenceDiskReport,
) -> usize {
    unsafe { report.as_ref() }.map_or(0, |report| report.volumes.len())
}

/// How many volumes carry a filesystem the host actually read. Distinct
/// from the composed count on purpose: an unrecognized volume stays in the
/// report rather than vanishing to keep one number correct.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_readable_filesystem_volume_count(
    report: *const RemanenceDiskReport,
) -> usize {
    unsafe { report.as_ref() }.map_or(0, |report| {
        report
            .filesystems
            .iter()
            .filter(|filesystem| filesystem.kind.is_some() && filesystem.issues.is_empty())
            .count()
    })
}

/// A volume's opaque identity.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_volume_id(
    report: *const RemanenceDiskReport,
    index: usize,
) -> u64 {
    unsafe { volume_record_view(report, index) }.map_or(0, |volume| volume.id)
}

/// What this volume was composed from.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_volume_origin(
    report: *const RemanenceDiskReport,
    index: usize,
) -> RemanenceVolumeOrigin {
    unsafe { volume_record_view(report, index) }
        .map_or(RemanenceVolumeOrigin::WholeDevice, |volume| volume.origin)
}

/// How many regions this volume was composed from; 0 for a whole-device
/// volume.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_volume_origin_region_count(
    report: *const RemanenceDiskReport,
    index: usize,
) -> usize {
    unsafe { volume_record_view(report, index) }.map_or(0, |volume| volume.origin_regions.len())
}

/// The identity of one region this volume was composed from.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_volume_origin_region_id(
    report: *const RemanenceDiskReport,
    index: usize,
    region_index: usize,
) -> u64 {
    unsafe { volume_record_view(report, index) }.map_or(0, |volume| {
        volume
            .origin_regions
            .get(region_index)
            .copied()
            .unwrap_or(0)
    })
}

/// Where the volume starts, in bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_volume_start_bytes(
    report: *const RemanenceDiskReport,
    index: usize,
) -> u64 {
    unsafe { volume_record_view(report, index) }.map_or(0, |volume| volume.start_bytes)
}

/// How long the volume is, in bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_volume_length_bytes(
    report: *const RemanenceDiskReport,
    index: usize,
) -> u64 {
    unsafe { volume_record_view(report, index) }.map_or(0, |volume| volume.length_bytes)
}

/// How many evidence lines this volume's composition carries.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_volume_evidence_count(
    report: *const RemanenceDiskReport,
    index: usize,
) -> usize {
    unsafe { volume_record_view(report, index) }.map_or(0, |volume| volume.evidence.len())
}

/// One evidence line from this volume's composition.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_volume_evidence(
    report: *const RemanenceDiskReport,
    index: usize,
    evidence_index: usize,
) -> *const c_char {
    unsafe { volume_record_view(report, index) }.map_or(ptr::null(), |volume| {
        volume
            .evidence
            .get(evidence_index)
            .map_or(ptr::null(), |line| line.as_ptr())
    })
}

/// How many volumes filesystem recognition was attempted on. A refused
/// attempt is recorded here, at the seam that owns the refusal.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_filesystem_count(
    report: *const RemanenceDiskReport,
) -> usize {
    unsafe { report.as_ref() }.map_or(0, |report| report.filesystems.len())
}

/// A filesystem's opaque identity.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_filesystem_id(
    report: *const RemanenceDiskReport,
    index: usize,
) -> u64 {
    unsafe { filesystem_view(report, index) }.map_or(0, |filesystem| filesystem.id)
}

/// The identity of the volume this recognition was attempted on.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_filesystem_volume_id(
    report: *const RemanenceDiskReport,
    index: usize,
) -> u64 {
    unsafe { filesystem_view(report, index) }.map_or(0, |filesystem| filesystem.volume)
}

/// The recognized filesystem kind, or null where recognition was refused —
/// the issue then says why, and the volume still stands.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_filesystem_kind(
    report: *const RemanenceDiskReport,
    index: usize,
) -> *const c_char {
    unsafe { filesystem_view(report, index) }.map_or(ptr::null(), |filesystem| {
        filesystem
            .kind
            .as_ref()
            .map_or(ptr::null(), |kind| kind.as_ptr())
    })
}

/// Whether a filesystem answered the label question at all. False where
/// recognition was refused — there is then no filesystem to answer, which
/// is not the same as a volume that answered "unlabeled".
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_filesystem_label_answered(
    report: *const RemanenceDiskReport,
    index: usize,
) -> bool {
    unsafe { filesystem_view(report, index) }.is_some_and(|filesystem| filesystem.label.is_some())
}

/// The volume label, or null where the volume has none — the format's own
/// spelling of unlabeled already resolved, so no caller compares strings
/// to find that out. Null also where nothing answered;
/// `remanence_report_filesystem_label_answered` tells the two apart.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_filesystem_label(
    report: *const RemanenceDiskReport,
    index: usize,
) -> *const c_char {
    unsafe { filesystem_view(report, index) }.map_or(ptr::null(), |filesystem| {
        filesystem
            .label
            .as_ref()
            .and_then(|label| label.name.as_ref())
            .map_or(ptr::null(), |name| name.as_ptr())
    })
}

/// Which source decided the answer, or null where the volume carries no
/// such source at all. A source that exists and says unlabeled is named
/// here beside a null label.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_filesystem_label_answered_by(
    report: *const RemanenceDiskReport,
    index: usize,
) -> *const c_char {
    unsafe { filesystem_view(report, index) }.map_or(ptr::null(), |filesystem| {
        filesystem
            .label
            .as_ref()
            .and_then(|label| label.answered_by.as_ref())
            .map_or(ptr::null(), |source| source.as_ptr())
    })
}

/// How many sources this filesystem read for the label, kept beside the
/// answer as evidence (P4).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_filesystem_label_reading_count(
    report: *const RemanenceDiskReport,
    index: usize,
) -> usize {
    unsafe { filesystem_view(report, index) }.map_or(0, |filesystem| {
        filesystem
            .label
            .as_ref()
            .map_or(0, |label| label.readings.len())
    })
}

pub(crate) unsafe fn label_reading_view<'a>(
    report: *const RemanenceDiskReport,
    index: usize,
    reading_index: usize,
) -> Option<&'a LabelReadingView> {
    unsafe { filesystem_view(report, index) }?
        .label
        .as_ref()?
        .readings
        .get(reading_index)
}

/// One source's name, in the recognizing filesystem's own vocabulary.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_filesystem_label_reading_source(
    report: *const RemanenceDiskReport,
    index: usize,
    reading_index: usize,
) -> *const c_char {
    unsafe { label_reading_view(report, index, reading_index) }
        .map_or(ptr::null(), |reading| reading.source.as_ptr())
}

/// Whether the format gives this volume that field at all. False is the
/// third state — no such field — and is distinct from a field that is
/// present and blank.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_filesystem_label_reading_present(
    report: *const RemanenceDiskReport,
    index: usize,
    reading_index: usize,
) -> bool {
    unsafe { label_reading_view(report, index, reading_index) }
        .is_some_and(|reading| reading.stored.is_some())
}

/// What that source holds, as stored and less the format's own
/// fixed-width padding: the empty string where it is present and blank,
/// and null where there is no such field.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_filesystem_label_reading_stored(
    report: *const RemanenceDiskReport,
    index: usize,
    reading_index: usize,
) -> *const c_char {
    unsafe { label_reading_view(report, index, reading_index) }.map_or(ptr::null(), |reading| {
        reading
            .stored
            .as_ref()
            .map_or(ptr::null(), |stored| stored.as_ptr())
    })
}

/// The allocation unit size, where the filesystem states one.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_filesystem_cluster_bytes(
    report: *const RemanenceDiskReport,
    index: usize,
    value_out: *mut u64,
) -> bool {
    let Some(value) = (unsafe { filesystem_view(report, index) }).and_then(|fs| fs.cluster_bytes)
    else {
        return false;
    };
    if let Some(out) = unsafe { value_out.as_mut() } {
        *out = value;
    }
    true
}

/// The allocation unit count, where the filesystem states one.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_filesystem_cluster_count(
    report: *const RemanenceDiskReport,
    index: usize,
    value_out: *mut u64,
) -> bool {
    let Some(value) = (unsafe { filesystem_view(report, index) }).and_then(|fs| fs.cluster_count)
    else {
        return false;
    };
    if let Some(out) = unsafe { value_out.as_mut() } {
        *out = value;
    }
    true
}

/// Sectors per track as the filesystem's own structures declare it. A
/// filesystem declaration, which manufactures no physical drive.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_filesystem_sectors_per_track(
    report: *const RemanenceDiskReport,
    index: usize,
    value_out: *mut u16,
) -> bool {
    let Some(value) =
        (unsafe { filesystem_view(report, index) }).and_then(|fs| fs.sectors_per_track)
    else {
        return false;
    };
    if let Some(out) = unsafe { value_out.as_mut() } {
        *out = value;
    }
    true
}

/// Heads as the filesystem's own structures declare it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_filesystem_heads(
    report: *const RemanenceDiskReport,
    index: usize,
    value_out: *mut u16,
) -> bool {
    let Some(value) = (unsafe { filesystem_view(report, index) }).and_then(|fs| fs.heads) else {
        return false;
    };
    if let Some(out) = unsafe { value_out.as_mut() } {
        *out = value;
    }
    true
}

/// Cylinders, only where the derivation is exact. Never invented.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_filesystem_cylinders(
    report: *const RemanenceDiskReport,
    index: usize,
    value_out: *mut u64,
) -> bool {
    let Some(value) = (unsafe { filesystem_view(report, index) }).and_then(|fs| fs.cylinders)
    else {
        return false;
    };
    if let Some(out) = unsafe { value_out.as_mut() } {
        *out = value;
    }
    true
}

/// How many issues this recognition carries.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_filesystem_issue_count(
    report: *const RemanenceDiskReport,
    index: usize,
) -> usize {
    unsafe { filesystem_view(report, index) }.map_or(0, |filesystem| filesystem.issues.len())
}

/// One issue's stable category.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_filesystem_issue_category(
    report: *const RemanenceDiskReport,
    index: usize,
    issue_index: usize,
    category_out: *mut RemanenceErrorCategory,
) -> bool {
    let Some(issue) =
        (unsafe { filesystem_view(report, index) }).and_then(|fs| fs.issues.get(issue_index))
    else {
        return false;
    };
    if let Some(out) = unsafe { category_out.as_mut() } {
        *out = issue.category;
    }
    true
}

/// One issue's diagnostic.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_report_filesystem_issue(
    report: *const RemanenceDiskReport,
    index: usize,
    issue_index: usize,
) -> *const c_char {
    unsafe { filesystem_view(report, index) }.map_or(ptr::null(), |filesystem| {
        filesystem
            .issues
            .get(issue_index)
            .map_or(ptr::null(), |issue| issue.message.as_ptr())
    })
}
