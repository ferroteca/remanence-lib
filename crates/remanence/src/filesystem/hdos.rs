// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! HDOS directory parsing for raw hard-sectored images (e.g. `.h8d`).

use crate::error::{Error, ErrorCategory, Result};
use crate::filesystem::space::path_is_root;
use crate::filesystem::{Catalog, Entry, EntryFact, EntryKind};

const SECTOR_SIZE: usize = 256;
const LABEL_SECTOR: usize = 9;
const DIR_ENTRY_SIZE: usize = 23;
const ENTRIES_PER_CLUSTER: usize = 22;
const DELETED_OR_EMPTY: u8 = 255;
const END_OF_DIRECTORY: u8 = 254;

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// One file listed in an HDOS directory (HDOS has no subdirectories).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HdosFile {
    pub name: String,
    pub extension: String,
    pub size_sectors: u32,
    pub modified_date: u16,
    pub flags: u8,
}

impl HdosFile {
    /// `"NAME.EXT"` or `"NAME"` when the extension is empty.
    pub fn display_name(&self) -> String {
        if self.extension.is_empty() {
            self.name.clone()
        } else {
            format!("{}.{}", self.name, self.extension)
        }
    }

    /// Size in bytes (`size_sectors * 256`).
    pub fn size_bytes(&self) -> u64 {
        u64::from(self.size_sectors) * SECTOR_SIZE as u64
    }

    /// HDOS flag letters: S/L/W/C.
    pub fn flags_string(&self) -> String {
        let mut text = String::new();
        if self.flags & 0x80 != 0 {
            text.push('S');
        }
        if self.flags & 0x40 != 0 {
            text.push('L');
        }
        if self.flags & 0x20 != 0 {
            text.push('W');
        }
        if self.flags & 0x10 != 0 {
            text.push('C');
        }
        text
    }

    /// HDOS catalog date, e.g. `"09-May-78"`, or `"No-Date"`.
    pub fn modified_date_string(&self) -> String {
        if self.modified_date == 0 {
            return "No-Date".to_owned();
        }

        let day = self.modified_date & 0x1f;
        let month = (self.modified_date >> 5) & 0xf;
        let year = ((self.modified_date >> 9) & 0x7f) + 70;
        if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
            return "No-Date".to_owned();
        }

        format!(
            "{day:02}-{}-{:02}",
            MONTHS[usize::from(month) - 1],
            year % 100
        )
    }
}

fn read_le16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

struct Label {
    dir_sector: u16,
    grt_sector: u16,
    sectors_per_group: u8,
    volume_sectors: u16,
}

fn parse_label(image: &[u8]) -> Result<Label> {
    if image.len() < (LABEL_SECTOR + 1) * SECTOR_SIZE {
        return Err(Error::invalid_image(
            "hdos",
            "image too small for HDOS label sector",
        ));
    }

    let label = &image[LABEL_SECTOR * SECTOR_SIZE..(LABEL_SECTOR + 1) * SECTOR_SIZE];
    let mut info = Label {
        dir_sector: read_le16(label, 3),
        grt_sector: read_le16(label, 5),
        sectors_per_group: label[7],
        volume_sectors: read_le16(label, 12),
    };
    let init_ver = label[9];
    let mut sector_size = read_le16(label, 14);

    let early_ver = matches!(init_ver, 0x00 | 0x10 | 0x15 | 0x16);
    if early_ver && info.volume_sectors == 0 {
        info.volume_sectors = 400;
    }
    if early_ver && (sector_size == 0 || sector_size == 255) {
        sector_size = 256;
    }

    if !matches!(info.sectors_per_group, 2 | 4 | 8) {
        return Err(Error::invalid_image(
            "hdos",
            "unsupported sectors-per-group in HDOS label",
        ));
    }
    if !matches!(info.volume_sectors, 400 | 800 | 1600) {
        return Err(Error::categorized_image(
            crate::ErrorCategory::Unsupported,
            "hdos",
            "unsupported HDOS volume size",
        ));
    }
    if sector_size != 256 {
        return Err(Error::categorized_image(
            crate::ErrorCategory::Unsupported,
            "hdos",
            "unsupported HDOS sector size",
        ));
    }
    if image.len() < usize::from(info.volume_sectors) * SECTOR_SIZE {
        return Err(Error::invalid_image(
            "hdos",
            "image shorter than HDOS volume size",
        ));
    }
    if info.dir_sector >= info.volume_sectors || info.grt_sector >= info.volume_sectors {
        return Err(Error::invalid_image(
            "hdos",
            "directory or GRT sector out of range",
        ));
    }

    Ok(info)
}

fn sector_bytes(image: &[u8], sector: u16, volume_sectors: u16) -> Result<&[u8]> {
    if sector >= volume_sectors {
        return Err(Error::invalid_image(
            "hdos",
            "sector reference out of range",
        ));
    }
    let offset = usize::from(sector) * SECTOR_SIZE;
    Ok(&image[offset..offset + SECTOR_SIZE])
}

fn trim_field(field: &[u8]) -> String {
    field
        .iter()
        .take_while(|&&byte| byte != 0)
        .map(|&byte| byte as char)
        .collect()
}

fn file_size_sectors(
    grt: &[u8],
    first_group: u8,
    last_group: u8,
    last_sector_index: u8,
    sectors_per_group: u8,
    num_clusters: u16,
) -> Option<u32> {
    let mut size: u32 = 0;
    let mut pos = u16::from(first_group);
    let mut hops: u16 = 0;
    while pos != u16::from(last_group) && hops < num_clusters {
        if pos >= num_clusters {
            return None;
        }
        size += u32::from(sectors_per_group);
        // Group references are single bytes, so `pos` always indexes within
        // the GRT sector even when the label claims more clusters.
        pos = u16::from(grt[usize::from(pos)]);
        hops += 1;
    }
    if hops >= num_clusters || pos != u16::from(last_group) {
        return None;
    }
    size += u32::from(last_sector_index);
    Some(size)
}

/// Reads a cataloged HDOS file's contents out through its GRT chain.
///
/// `name` is matched against each file's display name ("NAME.EXT"),
/// ASCII-case-insensitively.
pub fn read_hdos_file(image: &[u8], name: &str) -> Result<Vec<u8>> {
    let label = parse_label(image)?;
    let num_clusters = label.volume_sectors / u16::from(label.sectors_per_group);
    let grt_sector = sector_bytes(image, label.grt_sector, label.volume_sectors)?;
    let grt = &grt_sector[..usize::from(num_clusters).min(SECTOR_SIZE)];

    let mut cluster_sector = label.dir_sector;
    let mut depth = 0;
    while depth < 64 && cluster_sector != 0 {
        let first = sector_bytes(image, cluster_sector, label.volume_sectors)?;
        let second = sector_bytes(image, cluster_sector + 1, label.volume_sectors)?;
        let mut cluster = [0u8; SECTOR_SIZE * 2];
        cluster[..SECTOR_SIZE].copy_from_slice(first);
        cluster[SECTOR_SIZE..].copy_from_slice(second);

        let mut end_of_directory = false;
        for i in 0..ENTRIES_PER_CLUSTER {
            let entry = &cluster[i * DIR_ENTRY_SIZE..(i + 1) * DIR_ENTRY_SIZE];
            if entry[0] == END_OF_DIRECTORY {
                end_of_directory = true;
                break;
            }
            if entry[0] == DELETED_OR_EMPTY {
                continue;
            }

            let file_name = trim_field(&entry[..8]);
            let extension = trim_field(&entry[8..11]);
            let display = if extension.is_empty() {
                file_name.clone()
            } else {
                format!("{file_name}.{extension}")
            };
            if !display.eq_ignore_ascii_case(name) {
                continue;
            }

            // Walk the GRT chain, collecting each group's sectors; the
            // final group contributes only up to last_sector_index.
            let first_group = entry[16];
            let last_group = entry[17];
            let last_sector_index = u64::from(entry[18]);
            let mut contents = Vec::new();
            let mut pos = u16::from(first_group);
            let mut hops: u16 = 0;
            loop {
                let is_last = pos == u16::from(last_group);
                if !is_last && hops >= num_clusters {
                    return Err(Error::invalid_image(
                        "hdos",
                        format!("corrupt GRT chain for file {display}"),
                    ));
                }
                if pos >= num_clusters {
                    return Err(Error::invalid_image(
                        "hdos",
                        format!("corrupt GRT chain for file {display}"),
                    ));
                }
                let group_sectors = if is_last {
                    last_sector_index
                } else {
                    u64::from(label.sectors_per_group)
                };
                let base = u64::from(pos) * u64::from(label.sectors_per_group);
                for sector in 0..group_sectors {
                    let absolute = base + sector;
                    let bytes = sector_bytes(
                        image,
                        u16::try_from(absolute).map_err(|_| {
                            Error::invalid_image(
                                "hdos",
                                format!("corrupt GRT chain for file {display}"),
                            )
                        })?,
                        label.volume_sectors,
                    )?;
                    contents.extend_from_slice(bytes);
                }
                if is_last {
                    break;
                }
                pos = u16::from(grt[usize::from(pos)]);
                hops += 1;
            }
            return Ok(contents);
        }

        let next_cluster = read_le16(&cluster, 510);
        if end_of_directory || next_cluster == 0 {
            break;
        }
        cluster_sector = next_cluster;
        depth += 1;
    }

    Err(Error::categorized_image(
        crate::ErrorCategory::NotFound,
        "hdos",
        format!("file '{name}' not found"),
    ))
}

/// Parse the HDOS directory from a raw hard-sectored image (e.g. `.h8d`).
///
/// Returns a flat file list. Deleted and unused directory slots are omitted.
pub fn list_hdos_files(image: &[u8]) -> Result<Vec<HdosFile>> {
    let label = parse_label(image)?;

    let num_clusters = label.volume_sectors / u16::from(label.sectors_per_group);
    let grt_sector = sector_bytes(image, label.grt_sector, label.volume_sectors)?;
    // The GRT occupies a single sector; group references are single bytes, so
    // only the first 256 entries are ever reachable.
    let grt = &grt_sector[..usize::from(num_clusters).min(SECTOR_SIZE)];

    let mut files = Vec::new();
    let mut cluster_sector = label.dir_sector;
    let mut depth = 0;
    while depth < 64 && cluster_sector != 0 {
        let first = sector_bytes(image, cluster_sector, label.volume_sectors)?;
        let second = sector_bytes(image, cluster_sector + 1, label.volume_sectors)?;

        let mut cluster = [0u8; SECTOR_SIZE * 2];
        cluster[..SECTOR_SIZE].copy_from_slice(first);
        cluster[SECTOR_SIZE..].copy_from_slice(second);

        let mut end_of_directory = false;
        for i in 0..ENTRIES_PER_CLUSTER {
            let entry = &cluster[i * DIR_ENTRY_SIZE..(i + 1) * DIR_ENTRY_SIZE];
            if entry[0] == END_OF_DIRECTORY {
                end_of_directory = true;
                break;
            }
            if entry[0] == DELETED_OR_EMPTY {
                continue;
            }

            let mut file = HdosFile {
                name: trim_field(&entry[..8]),
                extension: trim_field(&entry[8..11]),
                flags: entry[14],
                modified_date: read_le16(entry, 21),
                ..HdosFile::default()
            };
            let first_group = entry[16];
            let last_group = entry[17];
            let last_sector_index = entry[18];

            let size = file_size_sectors(
                grt,
                first_group,
                last_group,
                last_sector_index,
                label.sectors_per_group,
                num_clusters,
            )
            .ok_or_else(|| {
                Error::invalid_image(
                    "hdos",
                    format!("corrupt GRT chain for file {}", file.display_name()),
                )
            })?;
            file.size_sectors = size;
            files.push(file);
        }

        let next_cluster = read_le16(&cluster, 510);
        if end_of_directory || next_cluster == 0 {
            break;
        }
        cluster_sector = next_cluster;
        depth += 1;
    }

    Ok(files)
}

/// The HDOS catalog, read out of the medium it occupies.
///
/// HDOS has no subdirectories, so the namespace is one root of leaves:
/// any path but the root is refused where it is asked to hold names, and
/// answered as absent where it is asked for one.
#[derive(Debug)]
pub(crate) struct HdosCatalog {
    /// The medium's bytes, held for as long as the view is (P27): the
    /// format bounds an HDOS volume small, and reading one file out of
    /// it is a walk of the group table rather than a second pass over
    /// the medium.
    image: Vec<u8>,
    files: Vec<crate::filesystem::hdos::HdosFile>,
}

/// The largest extent the HDOS reader will take whole (P27).
///
/// The bound is the adapter's own, and it is the whole of what bounds a
/// declared reading: a declaration names the adapter, and the adapter
/// says how much of an extent it is willing to hold.
const HDOS_BOUND: u64 = 8 * 1024 * 1024;

impl HdosCatalog {
    pub(crate) fn open(volume: &mut dyn crate::io::device::Device) -> Result<Self> {
        let length = volume.len();
        if length > HDOS_BOUND {
            return Err(Error::categorized_image(
                ErrorCategory::Unsupported,
                "hdos",
                format!(
                    "this extent is {length} bytes and the HDOS reader is \
                     bounded to {HDOS_BOUND}"
                ),
            ));
        }
        let mut image = vec![0u8; length as usize];
        volume.read_at(0, &mut image)?;
        let files = crate::filesystem::hdos::list_hdos_files(&image)?;
        Ok(Self { image, files })
    }

    fn entry(file: &crate::filesystem::hdos::HdosFile) -> Entry {
        Entry {
            name: file.display_name(),
            kind: EntryKind::File,
            size_bytes: file.size_bytes(),
            declared: vec![
                EntryFact::new("size-sectors", file.size_sectors.to_string()),
                EntryFact::new("modified-date", file.modified_date_string()),
                EntryFact::new("modified-date-raw", file.modified_date.to_string()),
                EntryFact::new("flags", file.flags_string()),
                EntryFact::new("flags-raw", file.flags.to_string()),
            ],
        }
    }
}

impl Catalog for HdosCatalog {
    fn entries(&self, path: &str) -> Result<Vec<Entry>> {
        if !path_is_root(path) {
            return Err(Error::categorized_image(
                ErrorCategory::NotDirectory,
                "hdos",
                format!("'{path}' holds no names; the HDOS catalog is flat"),
            ));
        }
        Ok(self.files.iter().map(Self::entry).collect())
    }

    fn stat(&self, path: &str) -> Result<Option<Entry>> {
        Ok(self
            .files
            .iter()
            .find(|file| file.display_name().eq_ignore_ascii_case(path))
            .map(Self::entry))
    }

    fn read_file(&self, path: &str) -> Result<Vec<u8>> {
        crate::filesystem::hdos::read_hdos_file(&self.image, path)
    }
}
