/*
 * SPDX-FileCopyrightText: 2026 Paul Galbraith
 * SPDX-License-Identifier: GPL-3.0-only
 */

/* Example C++ consumer, beside the C one (S2).
 *
 * `identify.c` is the same library met through the C ABI directly, and
 * it is the longer program for a reason worth seeing side by side: every
 * fallible call there carries three out-parameters and two strings to
 * free, and every handle has a `_free` a `goto` has to reach. Here the
 * destructors do that, the refusals arrive as one exception type caught
 * once at the bottom, and what is left is the journey itself.
 *
 * Build (MinGW, from the workspace root, after `cargo build -p remanence-ffi`):
 *   g++ -std=c++17 crates/remanence-ffi/examples/identify.cpp \
 *       target/debug/remanence_ffi.dll \
 *       -I crates/remanence-ffi/include -o identify.exe
 *
 * Usage:
 *   identify <path> [device-type]   what an artifact is, and what is on it
 *   identify --author [kind]        author a blank medium and write to it
 *   identify --devices              the devices and formats this release claims
 */

#include <remanence.hpp>

#include <cstdint>
#include <iomanip>
#include <iostream>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

namespace {

std::string_view name_of(remanence::LayerKind kind)
{
    switch (kind) {
    case remanence::LayerKind::Archive:
        return "archive";
    case remanence::LayerKind::Image:
        return "image";
    case remanence::LayerKind::PhysicalMedia:
        return "physical-media";
    case remanence::LayerKind::Filesystem:
        return "filesystem";
    case remanence::LayerKind::Unknown:
        break;
    }
    return "unknown";
}

std::string_view name_of(remanence::DiskContent content)
{
    switch (content) {
    case remanence::DiskContent::Blank:
        return "blank";
    case remanence::DiskContent::Schema:
        return "schema";
    case remanence::DiskContent::DirectVolume:
        return "direct-volume";
    case remanence::DiskContent::UnknownNonblank:
        break;
    }
    return "unknown-nonblank";
}

/// An absent answer printed as one, rather than as an empty line that
/// reads like a blank value.
std::string said(const std::optional<std::string_view>& value)
{
    return value.has_value() ? std::string{*value} : std::string{"(none)"};
}

void show_geometry(const remanence::Geometry& geometry)
{
    std::cout << "  geometry:\n";
    if (const std::optional<remanence::Coordinates> settled = geometry.coordinates()) {
        std::cout << "    " << settled->cylinders << " cylinders, " << settled->heads
                  << " heads, " << settled->sectors_per_track << " sectors/track, "
                  << settled->sector_bytes << " bytes/sector\n";
    } else {
        // Unstated is "nothing spoke"; undetermined is "two sources
        // disagreed" — different facts, and the library keeps them apart.
        std::cout << "    not settled ("
                  << (geometry.state() == remanence::GeometryState::Unstated ? "unstated"
                                                                            : "undetermined")
                  << ")\n";
        for (std::string_view part : geometry.conflicts()) {
            std::cout << "      conflict: " << part << '\n';
        }
        for (std::string_view part : geometry.unsettled()) {
            std::cout << "      unstated: " << part << '\n';
        }
    }
    for (const remanence::GeometryReading& reading : geometry.readings()) {
        std::cout << "    read from " << reading.source() << " at " << said(reading.at()) << '\n';
    }
}

void show_identification(const remanence::Identification& identification)
{
    std::cout << "  layers:\n";
    for (const remanence::Layer& layer : identification.layers()) {
        std::cout << "    " << name_of(layer.kind()) << ' ' << said(layer.id()) << " ("
                  << unsigned{layer.confidence()} << "%)";
        if (const std::optional<std::uint64_t> current = layer.current_bytes()) {
            std::cout << ", " << *current << " bytes";
        }
        std::cout << '\n';
    }
    std::cout << "  evidence:\n";
    for (std::string_view line : identification.evidence()) {
        std::cout << "    " << line << '\n';
    }
}

void show_report(const remanence::DiskReport& report)
{
    std::cout << "  content: " << name_of(report.content()) << " ("
              << said(report.content_evidence()) << ")\n";
    if (report.has_partition_schema()) {
        std::cout << "  schema: " << said(report.partition_schema_kind()) << '\n';
    }
    for (const remanence::ReportRegion& region : report.regions()) {
        std::cout << "    region " << region.declared_number() << ": "
                  << said(region.declared_type_reading()) << ", " << region.length_bytes()
                  << " bytes" << (region.is_claimed() ? ", claimed" : ", unclaimed") << '\n';
        if (const std::optional<std::string_view> issue = region.issue()) {
            std::cout << "      issue: " << *issue << '\n';
        }
    }
    for (const remanence::ReportFilesystem& filesystem : report.filesystems()) {
        std::cout << "    filesystem " << said(filesystem.kind()) << " on volume "
                  << filesystem.volume_id() << ", label " << said(filesystem.label()) << '\n';
    }
}

/// Walks whatever namespace the medium bears, one level deep.
void show_namespaces(remanence::Medium& medium)
{
    for (std::uint32_t ordinal : medium.partition_ordinals()) {
        std::optional<remanence::Partition> partition = medium.partition(ordinal);
        if (!partition.has_value() || !partition->bears_namespace()) {
            continue;
        }

        // Both doors may be opened off one partition; both compose the
        // same node.
        remanence::Filesystem filesystem = partition->filesystem();
        std::cout << "  partition " << ordinal << ": " << said(filesystem.kind());
        if (const std::optional<std::string> label = filesystem.label()) {
            std::cout << " labelled \"" << *label << '"';
        }
        std::cout << '\n';

        remanence::EntryList entries = filesystem.entries();
        for (const remanence::Entry& entry : entries.entries()) {
            std::cout << "      "
                      << (entry.kind() == remanence::EntryKind::Directory ? "d " : "  ")
                      << std::left << std::setw(16) << std::string{entry.name()} << std::right
                      << std::setw(10) << entry.size_bytes();
            for (const remanence::DeclaredFact& fact : entry.declared()) {
                std::cout << "  " << fact.key << '=' << fact.value;
            }
            std::cout << '\n';
        }
    }
}

int identify(const std::string& path, const std::optional<std::string>& device_type)
{
    remanence::Session session;

    // What the artifact is, before any machine is configured for it.
    // The claim is taken here and travels into the load, so nothing is
    // opened twice.
    remanence::Discovery discovery = remanence::discover_media(path);
    std::cout << said(discovery.path()) << '\n'
              << "  format: " << said(discovery.image_format()) << " ("
              << said(discovery.image_format_name()) << ")\n"
              << "  article: " << said(discovery.article()) << '\n'
              << "  recorded by: " << said(discovery.device_type()) << '\n'
              << "  size: " << discovery.size() << " bytes (" << discovery.image_size_bytes()
              << " on the raw plane)\n";
    std::cout << "  accepted by:";
    for (std::string_view device : discovery.accepting_devices()) {
        std::cout << ' ' << device;
    }
    std::cout << '\n';

    // A format recording several device types leaves the choice to the
    // caller: the discovery answers nothing for "what wrote it", and the
    // load takes the declaration instead.
    remanence::Medium medium =
        device_type.has_value()
            ? session.load_discovery_as(std::move(discovery), *device_type)
            : session.load_discovery(std::move(discovery));

    show_identification(medium.identify());
    show_geometry(medium.geometry());

    remanence::Assurance assurance = medium.assurance();
    std::cout << "  assurance: "
              << (assurance.outcome() == remanence::AssuranceOutcome::Verified ? "verified"
                                                                               : "degraded")
              << ", claim "
              << (assurance.claim() == remanence::Claim::LibraryOpened ? "library-opened"
                                                                       : "caller-opened")
              << '\n';

    show_report(medium.inspect());
    show_namespaces(medium);
    return 0;
}

int author(const std::string& kind)
{
    std::cout << "authored kinds:\n";
    for (const remanence::NewMediaKind& claimed : remanence::new_media_kinds()) {
        std::cout << "  " << claimed.id << " — " << claimed.name << " (" << claimed.article << ")"
                  << (claimed.takes_geometry ? ", takes coordinates" : "") << '\n';
    }

    // Authorship is the third fact class: nothing is discovered, because
    // there is no artifact. What the author states becomes the medium's
    // own facts.
    remanence::Session session;
    remanence::Medium blank = kind == "chs-disk" ? session.new_media(kind, 40, 2, 9, 512)
                                                 : session.new_media(kind);

    std::cout << '\n'
              << kind << ": " << said(blank.article()) << ", " << blank.size() << " bytes\n"
              << "  recorded by: " << said(blank.device_type()) << " (nothing did)\n";
    show_geometry(blank.geometry());

    for (std::string_view line : blank.assurance().evidence()) {
        std::cout << "  provenance: " << line << '\n';
    }

    if (blank.geometry().coordinates().has_value()) {
        // Session-backed until an explicit encode gives it an artifact,
        // so the commit point is ordinary and touches no file.
        std::vector<std::uint8_t> boot(512, 0x00);
        boot[510] = 0x55;
        boot[511] = 0xaa;
        blank.write_sector(0, 0, 1, boot);
        blank.commit();
        std::cout << "  wrote and committed a boot sector; it reads back "
                  << (blank.read_sector(0, 0, 1, boot.size()) == boot ? "identically"
                                                                      : "differently")
                  << '\n';
    }
    return 0;
}

int devices()
{
    std::cout << "devices:\n";
    for (const remanence::DeviceSlot& slot : remanence::device_slots()) {
        std::cout << "  " << std::left << std::setw(16) << std::string{slot.id} << std::right
                  << ' ' << slot.name << " [" << said(slot.device_class) << ", "
                  << said(slot.addressing) << "]\n";
    }
    std::cout << "formats:\n";
    for (const remanence::Format& format : remanence::formats()) {
        std::cout << "  " << std::left << std::setw(16) << std::string{format.id} << std::right
                  << ' ' << format.name;
        if (!format.device_types.empty()) {
            std::cout << " — records";
            for (std::string_view device : format.device_types) {
                std::cout << ' ' << device;
            }
        }
        std::cout << '\n';
    }
    return 0;
}

} // namespace

int main(int argc, char** argv)
{
    if (argc < 2) {
        std::cerr << "usage: identify <path> [device-type]\n"
                     "       identify --author [kind]\n"
                     "       identify --devices\n";
        return 2;
    }

    // One handler for the whole program: every refusal arrives here as
    // one type, carrying the stable category an embedder maps behaviour
    // from and the rule identity where an enumerated set owns one.
    try {
        const std::string first{argv[1]};
        if (first == "--devices") {
            return devices();
        }
        if (first == "--author") {
            return author(argc > 2 ? std::string{argv[2]} : std::string{"chs-disk"});
        }
        return identify(first, argc > 2 ? std::optional<std::string>{argv[2]} : std::nullopt);
    } catch (const remanence::Error& refusal) {
        std::cerr << "refused (category " << static_cast<int>(refusal.category());
        if (refusal.rule().has_value()) {
            std::cerr << ", rule " << *refusal.rule();
        }
        std::cerr << "): " << refusal.what() << '\n';
        return 1;
    } catch (const std::exception& other) {
        std::cerr << "failed: " << other.what() << '\n';
        return 1;
    }
}
