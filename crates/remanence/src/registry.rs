// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

use std::collections::BTreeMap;
use std::path::Path;

use crate::error::{Error, Result};

/// A container format definition (e.g. a raw floppy image layout).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ContainerFormat {
    pub id: String,
    pub name: String,
    pub extensions: Vec<String>,
    pub media_kind: Option<String>,
    pub sector_size: Option<usize>,
    pub cylinders: Option<usize>,
    pub sides: Option<usize>,
    pub tracks: Option<usize>,
    pub sectors_per_track: Option<usize>,
    /// A leading byte signature, hex-encoded in the definition text.
    pub magic: Option<Vec<u8>>,
    pub filesystem_candidates: Vec<String>,
    /// Unknown keys are preserved here so the schema can grow gradually.
    pub attributes: BTreeMap<String, String>,
}

impl ContainerFormat {
    /// `sector_size * cylinders_or_tracks * sides.unwrap_or(1) * sectors_per_track`.
    pub fn expected_size(&self) -> Option<usize> {
        let sector_size = self.sector_size?;
        let cylinders = self.cylinders_or_tracks()?;
        let sectors_per_track = self.sectors_per_track?;
        Some(sector_size * cylinders * self.sides.unwrap_or(1) * sectors_per_track)
    }

    pub fn cylinders_or_tracks(&self) -> Option<usize> {
        self.cylinders.or(self.tracks)
    }

    pub fn sides_value(&self) -> Option<usize> {
        self.sides
    }
}

/// A filesystem format definition and its detection heuristics.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FilesystemFormat {
    pub id: String,
    pub name: String,
    pub aliases: Vec<String>,
    pub container_candidates: Vec<String>,
    pub heuristics: Vec<String>,
    pub markers: Vec<String>,
    /// Unknown keys are preserved here so the schema can grow gradually.
    pub attributes: BTreeMap<String, String>,
}

/// Parsed container and filesystem format definitions, keyed by id.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FormatRegistry {
    containers: BTreeMap<String, ContainerFormat>,
    filesystems: BTreeMap<String, FilesystemFormat>,
}

impl FormatRegistry {
    pub fn parse(container_formats: &str, filesystem_formats: &str) -> Result<Self> {
        let mut builder = RegistryBuilder::default();
        builder.parse_container_formats(container_formats)?;
        builder.parse_filesystem_formats(filesystem_formats)?;
        Ok(builder.finish())
    }

    pub fn from_files(
        container_formats_path: &Path,
        filesystem_formats_path: &Path,
    ) -> Result<Self> {
        let read_file = |path: &Path| -> Result<String> {
            let bytes = std::fs::read(path)
                .map_err(|_| Error::io(format!("failed to open '{}'", path.display())))?;
            Ok(String::from_utf8_lossy(&bytes).into_owned())
        };

        let container_formats = read_file(container_formats_path)?;
        let filesystem_formats = read_file(filesystem_formats_path)?;
        Self::parse(&container_formats, &filesystem_formats)
    }

    pub fn container(&self, id: &str) -> Option<&ContainerFormat> {
        self.containers.get(id)
    }

    pub fn filesystem(&self, id: &str) -> Option<&FilesystemFormat> {
        self.filesystems.get(id)
    }

    pub fn containers(&self) -> &BTreeMap<String, ContainerFormat> {
        &self.containers
    }

    pub fn filesystems(&self) -> &BTreeMap<String, FilesystemFormat> {
        &self.filesystems
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DefinitionKind {
    Container,
    Filesystem,
}

impl DefinitionKind {
    fn name(self) -> &'static str {
        match self {
            Self::Container => "container",
            Self::Filesystem => "filesystem",
        }
    }
}

enum Section {
    Container(String),
    Filesystem(String),
}

fn strip_comment(line: &str) -> &str {
    match line.find('#') {
        Some(pos) => &line[..pos],
        None => line,
    }
}

fn parse_list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|piece| !piece.is_empty())
        .map(str::to_owned)
        .collect()
}

fn parse_usize(value: &str, line: usize) -> Result<usize> {
    let error = || {
        Error::registry(line, format!("expected unsigned integer, found '{value}'"))
    };
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(error());
    }
    value.parse().map_err(|_| error())
}

fn parse_hex_bytes(value: &str, line: usize) -> Result<Vec<u8>> {
    let error = || {
        Error::registry(
            line,
            format!("expected an even-length hex byte string, found '{value}'"),
        )
    };
    if value.is_empty() || value.len() % 2 != 0 {
        return Err(error());
    }
    (0..value.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(&value[at..at + 2], 16).map_err(|_| error()))
        .collect()
}

/// Returns `Ok(None)` when the line is not a section header at all.
fn parse_section_header(
    line: &str,
    line_number: usize,
    expected_kind: DefinitionKind,
) -> Result<Option<Section>> {
    if !line.starts_with('[') {
        return Ok(None);
    }
    if !line.ends_with(']') {
        return Err(Error::registry(line_number, "malformed section header"));
    }
    let section = &line[1..line.len() - 1];

    let (kind, id) = match section.find('.') {
        Some(dot) => (&section[..dot], &section[dot + 1..]),
        None => (expected_kind.name(), section),
    };

    if id.trim().is_empty() {
        return Err(Error::registry(line_number, "section id must not be empty"));
    }

    if kind != expected_kind.name() {
        return Err(Error::registry(
            line_number,
            format!(
                "expected {} section, found {} section",
                expected_kind.name(),
                kind
            ),
        ));
    }

    Ok(Some(match expected_kind {
        DefinitionKind::Container => Section::Container(id.to_owned()),
        DefinitionKind::Filesystem => Section::Filesystem(id.to_owned()),
    }))
}

#[derive(Default)]
struct RegistryBuilder {
    registry: FormatRegistry,
    current: Option<Section>,
}

impl RegistryBuilder {
    fn parse_container_formats(&mut self, input: &str) -> Result<()> {
        self.parse_definitions(input, DefinitionKind::Container)
    }

    fn parse_filesystem_formats(&mut self, input: &str) -> Result<()> {
        self.parse_definitions(input, DefinitionKind::Filesystem)
    }

    fn finish(self) -> FormatRegistry {
        self.registry
    }

    fn parse_definitions(&mut self, input: &str, kind: DefinitionKind) -> Result<()> {
        self.current = None;

        for (index, raw_line) in input.split('\n').enumerate() {
            let line_number = index + 1;
            let line = strip_comment(raw_line).trim();
            if line.is_empty() {
                continue;
            }

            match parse_section_header(line, line_number, kind)? {
                Some(section) => self.start_section(section, line_number)?,
                None => {
                    let Some(eq) = line.find('=') else {
                        return Err(Error::registry(line_number, "expected key = value"));
                    };
                    let key = line[..eq].trim();
                    let value = line[eq + 1..].trim();
                    self.set_value(key, value, line_number)?;
                }
            }
        }

        Ok(())
    }

    fn start_section(&mut self, section: Section, line: usize) -> Result<()> {
        match &section {
            Section::Container(id) => {
                if self.registry.containers.contains_key(id) {
                    return Err(Error::registry(line, format!("duplicate container '{id}'")));
                }
                self.registry.containers.insert(
                    id.clone(),
                    ContainerFormat { id: id.clone(), ..ContainerFormat::default() },
                );
            }
            Section::Filesystem(id) => {
                if self.registry.filesystems.contains_key(id) {
                    return Err(Error::registry(
                        line,
                        format!("duplicate filesystem '{id}'"),
                    ));
                }
                self.registry.filesystems.insert(
                    id.clone(),
                    FilesystemFormat { id: id.clone(), ..FilesystemFormat::default() },
                );
            }
        }

        self.current = Some(section);
        Ok(())
    }

    fn set_value(&mut self, key: &str, value: &str, line: usize) -> Result<()> {
        match &self.current {
            None => Err(Error::registry(line, "value found before a section header")),
            Some(Section::Container(id)) => {
                let container = self.registry.containers.get_mut(id).expect("current section");
                set_container_value(container, key, value, line)
            }
            Some(Section::Filesystem(id)) => {
                let filesystem =
                    self.registry.filesystems.get_mut(id).expect("current section");
                set_filesystem_value(filesystem, key, value);
                Ok(())
            }
        }
    }
}

fn set_container_value(
    container: &mut ContainerFormat,
    key: &str,
    value: &str,
    line: usize,
) -> Result<()> {
    match key {
        "name" => container.name = value.to_owned(),
        "extensions" => container.extensions = parse_list(value),
        "media_kind" => container.media_kind = Some(value.to_owned()),
        "sector_size" => container.sector_size = Some(parse_usize(value, line)?),
        "cylinders" => container.cylinders = Some(parse_usize(value, line)?),
        "sides" => container.sides = Some(parse_usize(value, line)?),
        "tracks" => container.tracks = Some(parse_usize(value, line)?),
        "sectors_per_track" => {
            container.sectors_per_track = Some(parse_usize(value, line)?)
        }
        "magic" => container.magic = Some(parse_hex_bytes(value, line)?),
        "filesystem_candidates" => container.filesystem_candidates = parse_list(value),
        _ => {
            container.attributes.insert(key.to_owned(), value.to_owned());
        }
    }
    Ok(())
}

fn set_filesystem_value(filesystem: &mut FilesystemFormat, key: &str, value: &str) {
    match key {
        "name" => filesystem.name = value.to_owned(),
        "aliases" => filesystem.aliases = parse_list(value),
        "container_candidates" => filesystem.container_candidates = parse_list(value),
        "heuristics" => filesystem.heuristics = parse_list(value),
        "markers" => filesystem.markers = parse_list(value),
        _ => {
            filesystem.attributes.insert(key.to_owned(), value.to_owned());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DEFAULT_CONTAINER_FORMATS, DEFAULT_FILESYSTEM_FORMATS};

    #[test]
    fn parses_container_and_filesystem_sections() {
        let registry =
            FormatRegistry::parse(DEFAULT_CONTAINER_FORMATS, DEFAULT_FILESYSTEM_FORMATS)
                .expect("default formats parse");

        let h8d = registry.container("h8d").expect("h8d container");
        assert_eq!(h8d.name, "Heathkit H8 H17 disk image");
        assert_eq!(h8d.expected_size(), Some(102_400));
        assert_eq!(h8d.filesystem_candidates, vec!["hdos", "cpm"]);

        let hdos = registry.filesystem("hdos").expect("hdos filesystem");
        assert_eq!(hdos.name, "Heath Disk Operating System");
        assert_eq!(hdos.heuristics, vec!["ascii-marker"]);
    }

    #[test]
    fn preserves_unknown_attributes_for_future_flexibility() {
        let registry = FormatRegistry::parse(
            "\n            [container.demo]\n            name = Demo\n\
             future_field = keep me\n            ",
            "",
        )
        .expect("demo definition parses");

        let demo = registry.container("demo").expect("demo container");
        assert_eq!(demo.attributes.get("future_field").map(String::as_str), Some("keep me"));
    }

    #[test]
    fn rejects_filesystem_sections_in_container_definitions() {
        let error = FormatRegistry::parse("[filesystem.hdos]\nname = HDOS\n", "")
            .expect_err("filesystem section rejected");
        assert!(error
            .to_string()
            .contains("expected container section, found filesystem section"));
    }
}
