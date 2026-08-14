/*
 * SPDX-FileCopyrightText: 2026 Paul Galbraith
 * SPDX-License-Identifier: GPL-3.0-only
 */

/*
 * The C++ presentation exercised as a C++ caller meets it (S2, D53).
 *
 * `abi_boundary.c` is the same idea one layer down: it crosses the C
 * boundary as a C caller does. This crosses it through
 * `<remanence.hpp>`, which is where a C++ consumer meets the library —
 * so what it checks is the wrapper's own claims: that a refusal arrives
 * as a typed exception carrying the delivered category, that an owned
 * handle frees itself and a moved-from one does not free twice, that a
 * view is a view, and that an absence comes back empty rather than
 * throwing.
 *
 * **It opens no disk image, deliberately.** Every fixture this project
 * tests against is third-party media it does not distribute, so the
 * journeys here make their own medium through the authorship door —
 * which is also the one fact class with no artifact behind it, and
 * therefore the one that always works on a fresh clone.
 *
 * It takes one group name and runs that group, so each group is a named
 * test on the Rust side rather than one pass-or-fail lump, and it keeps
 * going after a failed check so one run reports everything wrong with a
 * group.
 */

#include <remanence.hpp>

#include "worked_example.hpp"

#ifdef _WIN32
#include <windows.h>
#else
#include <fcntl.h>
#include <unistd.h>
#endif

#include <cstdio>
#include <cstring>
#include <string>
#include <utility>
#include <vector>

namespace {

int failures = 0;
int checks = 0;

void record(bool passed, const char* file, int line, const std::string& said)
{
    checks += 1;
    if (!passed) {
        failures += 1;
        std::printf("  FAIL %s:%d: %s\n", file, line, said.c_str());
    }
}

#define CHECK(condition, said) record((condition), __FILE__, __LINE__, (said))

/// Runs `body` and answers the refusal it threw, or reports that it did
/// not throw at all. A refusal is the point of most of these checks, so
/// "nothing was thrown" has to be a failure rather than a silent pass.
template <typename Body>
void check_refuses(const char* what, Body body, const char* file, int line)
{
    checks += 1;
    try {
        body();
    } catch (const remanence::Error& refusal) {
        if (std::strlen(refusal.what()) == 0) {
            failures += 1;
            std::printf("  FAIL %s:%d: %s refused with an empty diagnostic\n", file, line, what);
        }
        return;
    } catch (const std::exception& other) {
        failures += 1;
        std::printf("  FAIL %s:%d: %s threw something that is not a remanence::Error: %s\n", file,
                    line, what, other.what());
        return;
    }
    failures += 1;
    std::printf("  FAIL %s:%d: %s was not refused\n", file, line, what);
}

#define CHECK_REFUSES(what, body) check_refuses((what), (body), __FILE__, __LINE__)

/// The authored medium every journey below is made on: a CHS disk whose
/// coordinates are the author's own.
remanence::Medium blank_disk(remanence::Session& session)
{
    return session.new_media("chs-disk", 40, 2, 9, 512);
}

/* ------------------------------------------------------------ catalogs
 *
 * Pure lookups, and the cheapest thing that can only work if the
 * wrapper's vectors, views and enums cross correctly.
 */
void group_catalogs()
{
    CHECK(!remanence::version().empty(), "the library reported no version");
    CHECK(remanence::default_cache_bytes() > 0, "the default cache bound is zero");

    const std::vector<remanence::Format> formats = remanence::formats();
    CHECK(!formats.empty(), "the release claims no formats at all");
    for (const remanence::Format& format : formats) {
        CHECK(!format.id.empty(), "a format has an empty id");
        CHECK(!format.name.empty(), std::string{"format "} + std::string{format.id} + " has no name");
    }

    const std::vector<remanence::DeviceSlot> slots = remanence::device_slots();
    CHECK(!slots.empty(), "the release claims no device slots");
    bool receiver = false;
    for (const remanence::DeviceSlot& slot : slots) {
        CHECK(!slot.id.empty(), "a device slot has an empty id");
        // The archive receiver is the one slot that is no device type,
        // and it answers nothing for every device-type question.
        if (slot.id == "archive") {
            receiver = true;
            CHECK(!slot.device_class.has_value(),
                  "the archive receiver claimed a device class");
            CHECK(!slot.addressing.has_value(), "the archive receiver claimed an addressing");
        } else {
            CHECK(slot.addressing.has_value(),
                  std::string{"device type "} + std::string{slot.id} + " declares no addressing");
        }
    }
    CHECK(receiver, "no archive receiver among the slots");

    const std::vector<remanence::NewMediaKind> kinds = remanence::new_media_kinds();
    CHECK(!kinds.empty(), "the release authors no media at all");
    bool chs = false;
    for (const remanence::NewMediaKind& kind : kinds) {
        if (kind.id == "chs-disk") {
            chs = true;
            CHECK(kind.takes_geometry, "chs-disk does not take coordinates");
        }
    }
    CHECK(chs, "chs-disk is not among the authored kinds");

    CHECK(!remanence::partition_schemes().empty(), "the release claims no partition schemes");
    CHECK(!remanence::partition_types().empty(), "the release claims no partition types");
    CHECK(!remanence::assurance_conditions().empty(), "the release claims no assurance conditions");
    CHECK(!remanence::geometry_sources().empty(), "the release claims no geometry sources");
    CHECK(!remanence::dos_rules().empty(), "the release claims no DOS rules");
}

/* --------------------------------------------------------- refusals
 *
 * The whole of what this header promises about failure: a typed
 * exception, the delivered category on it, the rule identity where one
 * applies, and an ordinary `std::exception` for a caller who catches
 * broadly.
 */
void group_refusals()
{
    // A missing artifact: the category is delivered, not invented here.
    try {
        remanence::Discovery discovery = remanence::discover_media("no-such-artifact-anywhere.img");
        CHECK(!static_cast<bool>(discovery), "opening a missing artifact answered a discovery");
    } catch (const remanence::Error& refusal) {
        CHECK(std::strlen(refusal.what()) > 0, "a refusal carried an empty diagnostic");
        CHECK(refusal.category() == remanence::ErrorCategory::NotFound
                  || refusal.category() == remanence::ErrorCategory::Io,
              "a missing artifact was not classified as absent or as I/O");
    }

    // Catchable as a plain std::exception, which is what a caller with
    // one handler at the top of main() relies on.
    bool caught_broadly = false;
    try {
        remanence::discover_media("no-such-artifact-anywhere.img");
    } catch (const std::exception&) {
        caught_broadly = true;
    }
    CHECK(caught_broadly, "a refusal did not arrive as a std::exception");

    remanence::Session session;

    // An unknown authored kind, refused by name.
    CHECK_REFUSES("an unknown authored kind",
                  [&session] { session.new_media("no-such-kind-at-all"); });

    // Coordinates that address nothing, refused at the one moment
    // authorship offers.
    CHECK_REFUSES("coordinates that address nothing",
                  [&session] { session.new_media("chs-disk", 0, 2, 9, 512); });

    // A sector outside the authored coordinates.
    remanence::Medium blank = blank_disk(session);
    CHECK_REFUSES("a sector outside the authored coordinates",
                  [&blank]() mutable { blank.read_sector(999, 0, 1, 512); });

    // A partition with nothing recorded to check a reading against.
    std::optional<remanence::Partition> direct = blank.partition(0);
    CHECK(direct.has_value(), "an authored medium bears no direct partition");
    if (direct.has_value()) {
        CHECK_REFUSES("a type reading with nothing recorded to check it against",
                      [&direct] { direct->check_type("dos-primary"); });
    }

    // An authored blank assumes no device, so no drive takes it.
    remanence::StorageDevice drive = session.add_device("h17");
    const std::uint64_t media_id = blank.id();
    CHECK_REFUSES("seating an authored blank in a drive",
                  [&]() mutable { drive.insert(media_id); });

    // An index past the end is a C++ range error rather than the ABI's
    // null-or-zero answer.
    bool out_of_range = false;
    try {
        blank.partition_ordinal(blank.partition_count());
    } catch (const std::out_of_range&) {
        out_of_range = true;
    }
    CHECK(out_of_range, "an index past the end did not throw std::out_of_range");
}

/* ------------------------------------------------------- authorship
 *
 * The journey a consumer actually makes, with no artifact behind it:
 * state the facts, read them back as the medium's own, write a sector,
 * commit, and read it again.
 */
void group_authorship()
{
    remanence::Session session;
    remanence::Medium blank = blank_disk(session);

    CHECK(blank.article() == std::string_view{"authored"},
          "an authored medium is not the authored article");
    CHECK(blank.size() == 40ull * 2 * 9 * 512, "the authored size is not the coordinates' product");
    CHECK(!blank.device_type().has_value(), "an authored blank assumed a device");
    CHECK(!blank.is_linked(), "a fresh medium is linked to something");

    // The author's own coordinates, carried as the medium's geometry
    // with authorship as their one reading.
    remanence::Geometry geometry = blank.geometry();
    CHECK(geometry.state() == remanence::GeometryState::Determined,
          "the authored geometry is not determined");
    std::optional<remanence::Coordinates> coordinates = geometry.coordinates();
    CHECK(coordinates.has_value(), "the authored geometry states no coordinates");
    if (coordinates.has_value()) {
        CHECK(coordinates->cylinders == 40 && coordinates->heads == 2
                  && coordinates->sectors_per_track == 9 && coordinates->sector_bytes == 512,
              "the coordinates read back are not the ones stated");
    }
    CHECK(geometry.reading_count() == 1, "an authored medium's coordinates have one source");
    if (geometry.reading_count() == 1) {
        CHECK(geometry.reading(0).source() == std::string_view{"authorship"},
              "the one reading is not authorship");
    }
    CHECK(geometry.conflicts().empty(), "the authored geometry reports a conflict");

    // Nobody opened it, so the claim is the third class.
    remanence::Assurance assurance = blank.assurance();
    CHECK(assurance.outcome() == remanence::AssuranceOutcome::Verified,
          "an authored medium did not verify");
    CHECK(assurance.claim() == remanence::Claim::Authored,
          "an authored medium's claim is not authorship");
    CHECK(!assurance.evidence().empty(), "the assurance carries no evidence");

    // A sector written is buffered until it is committed (P2).
    std::vector<std::uint8_t> payload(512, 0xa5);
    payload[510] = 0x55;
    payload[511] = 0xaa;
    blank.write_sector(0, 0, 1, payload);
    CHECK(blank.is_modified(), "a written sector left the medium unmodified");
    blank.commit();
    CHECK(blank.read_sector(0, 0, 1, 512) == payload, "the sector did not read back as written");

    // And rollback discards without touching anything.
    blank.write_sector(1, 0, 1, std::vector<std::uint8_t>(512, 0x5a));
    blank.rollback();
    CHECK(blank.read_sector(1, 0, 1, 512) == std::vector<std::uint8_t>(512, 0x00),
          "a rolled-back sector kept its write");

    // A medium recording no scheme bears the direct partition, and the
    // report says the same thing from the other side.
    CHECK(!blank.partition_scheme().has_value(), "an authored medium recorded a scheme");
    CHECK(blank.partition_count() == 1, "an authored medium's pool is not one partition");
    std::optional<remanence::Partition> direct = blank.partition(0);
    CHECK(direct.has_value(), "the direct partition is not at ordinal 0");
    if (direct.has_value()) {
        CHECK(direct->is_direct(), "the one partition is not the direct one");
        CHECK(!direct->type_byte().has_value(), "the direct partition recorded a type byte");
        CHECK(direct->provenance().has_value(),
              "the direct partition carries no provenance for a composition it is");
    }

    // Inspection reads a medium the way an image format presents it,
    // and an authored blank has no such presentation until an explicit
    // encode gives it an artifact — so the refusal is the answer here,
    // and the report is exercised over a real one in `report` below.
    CHECK_REFUSES("inspecting a medium no format presents",
                  [&]() mutable { blank.inspect(); });

    // Release is the one verb that ends state.
    const std::uint64_t media_id = blank.id();
    CHECK(session.media_count() == 1, "the pool does not hold the authored medium");
    session.release_media(media_id);
    CHECK(session.media_count() == 0, "release left the medium pooled");
    CHECK(!session.medium(media_id).has_value(), "a released medium still answers");
}

/* ------------------------------------------------------------ views
 *
 * Machines and devices are configuration, and the wrapper presents them
 * as views because the ABI says the session owns them. What that has to
 * mean is that copying one costs nothing and destroying one releases
 * nothing.
 */
void group_views()
{
    remanence::Session session;

    remanence::StorageDevice drive = session.add_device("h17");
    CHECK(!drive.attachment().empty(), "a device has no attachment identity");
    CHECK(drive.slot() == std::string_view{"h17"}, "the device is not of the slot asked for");
    CHECK(!drive.is_occupied(), "a fresh device is occupied");
    CHECK(!drive.medium().has_value(), "an empty device answered a medium");
    const std::string attachment{drive.attachment()};

    {
        // A copy of a view, destroyed. If this released anything, every
        // check after it would fail.
        remanence::StorageDevice copy = drive;
        CHECK(copy.attachment() == drive.attachment(), "a copied view named a different device");
    }
    CHECK(session.device_count() == 1, "destroying a view released the device");

    std::optional<remanence::StorageDevice> again = session.device(attachment);
    CHECK(again.has_value(), "the session cannot find the device it just added");
    CHECK(!session.device("no-such-attachment").has_value(),
          "an unknown attachment answered a device");

    const std::vector<std::string> attachments = session.device_attachments();
    CHECK(attachments.size() == 1, "the session lists a different number of devices");

    // A named machine beside the anonymous one.
    remanence::Machine machine = session.add_machine("workbench");
    CHECK(machine.identity() == std::string_view{"workbench"}, "the machine has another identity");
    remanence::StorageDevice owned = machine.add_device("h17");
    CHECK(machine.device_count() == 1, "the machine did not take its device");
    CHECK(machine.device(std::string{owned.attachment()}).has_value(),
          "the machine cannot find its own device");
    CHECK(!session.machine("no-such-machine").has_value(), "an unknown machine answered");

    session.release_machine("workbench");
    CHECK(!session.machine("workbench").has_value(), "a released machine still answers");

    // Insert and eject are the one edge between configuration and
    // state, and ejecting takes nothing away.
    remanence::Medium blank = blank_disk(session);
    remanence::StorageDevice receiver = session.add_device("archive");
    CHECK(!receiver.device_type().has_value(), "the archive receiver claimed a device type");
    CHECK(session.media_count() == 1, "the authored medium is not pooled");
    (void)blank;
}

/* -------------------------------------------------------- lifetimes
 *
 * What RAII has to mean here: an owned handle frees itself, a moved-from
 * one is empty and frees nothing, and `release()` hands the handle to a
 * C caller who will free it instead. Whether anything actually leaked is
 * `wrapper_leaks.cpp`'s question — this one is about the shape.
 */
void group_lifetimes()
{
    remanence::Session session;
    remanence::Medium blank = blank_disk(session);

    remanence::Geometry geometry = blank.geometry();
    CHECK(static_cast<bool>(geometry), "a fresh owned handle is empty");

    remanence::Geometry moved = std::move(geometry);
    CHECK(static_cast<bool>(moved), "a move-constructed handle is empty");
    // NOLINTNEXTLINE(bugprone-use-after-move) — that it is empty is the check.
    CHECK(!static_cast<bool>(geometry), "a moved-from handle still holds its own");

    remanence::Geometry assigned = blank.geometry();
    assigned = std::move(moved);
    CHECK(static_cast<bool>(assigned), "a move-assigned handle is empty");
    CHECK(!static_cast<bool>(moved), "a moved-from handle still holds its own");

    // Handing a handle back to C: the wrapper stops owning it, and
    // freeing it is the C caller's from then on.
    remanence::Assurance assurance = blank.assurance();
    RemanenceAssurance* raw = assurance.release();
    CHECK(raw != nullptr, "release() answered nothing");
    CHECK(!static_cast<bool>(assurance), "release() left the wrapper owning the handle");
    CHECK(remanence_assurance_claim(raw) == REMANENCE_CLAIM_AUTHORED,
          "the released handle stopped answering");
    remanence_assurance_free(raw);

    // The raw handle a wrapper still owns, for the C functions this
    // header does not wrap — the flux doors among them.
    remanence::Identification identification = blank.identify();
    CHECK(remanence_identification_layer_count(identification.get())
              == identification.layer_count(),
          "the borrowed raw handle disagrees with its wrapper");

    // A default-constructed session is the only handle here that mints
    // itself, and its destructor is what ends the claim.
    {
        remanence::Session scoped;
        remanence::Medium inner = scoped.new_media("chs-disk", 2, 1, 8, 256);
        CHECK(scoped.media_count() == 1, "the scoped session did not pool its medium");
        (void)inner;
    }
}

/* ------------------------------------------------------- absences
 *
 * The ABI answers null for an honest absence in a good many places, and
 * the wrapper's promise is that those arrive as an empty optional while
 * only failures throw. Getting this backwards would make a wrapper that
 * throws on ordinary answers.
 */
void group_absences()
{
    remanence::Session session;
    remanence::Medium blank = blank_disk(session);

    CHECK(!blank.path().has_value(), "a medium with no artifact answered a path");
    CHECK(!blank.image_path().has_value(), "a medium with no artifact answered an image path");
    CHECK(!blank.format().has_value(), "an authored medium answered a container format");
    CHECK(!blank.partition_scheme().has_value(), "an authored medium answered a scheme");
    CHECK(!blank.partition(7).has_value(), "an ordinal the pool lacks answered a partition");
    CHECK(!session.medium(9999).has_value(), "an identity the pool lacks answered a medium");
    CHECK(!session.machine("nowhere").has_value(), "an identity no machine has answered one");

    remanence::Assurance assurance = blank.assurance();
    CHECK(!assurance.condition().has_value(), "a verified open named a withholding condition");

    remanence::Identification identification = blank.identify();
    CHECK(identification.layer_count() > 0, "an authored medium identified no layer at all");
    remanence::Layer layer = identification.layer(0);
    CHECK(layer.kind() != remanence::LayerKind::Archive, "an authored medium is inside an archive");

    // The direct partition bears no namespace this release recognizes,
    // so the namespace door refuses while the addressable one answers.
    std::optional<remanence::Partition> direct = blank.partition(0);
    CHECK(direct.has_value(), "an authored medium bears no direct partition");
    if (direct.has_value()) {
        CHECK(direct->is_addressable(), "the direct partition is not addressable");
        remanence::Volume volume = direct->volume();
        CHECK(volume.length_bytes() == blank.size(),
              "the direct volume is not the whole of the medium");
        std::vector<std::uint8_t> boot = volume.read_at(0, 16);
        CHECK(boot.size() == 16, "a bounded volume read answered a different length");
    }
}

/* --------------------------------------------------------- the flux ladder
 *
 * The other half of the ABI: a remanence image, the family's hardware
 * bitstream over it, the encoded bytestream above that, and the sectors
 * the recording states for itself — each a handle of its own, each
 * materialized from the one below.
 *
 * **The disk is built here rather than fetched**, from the format's own
 * worked example: one index hole at 3/8 of a turn and one orbit at
 * 57,150 µm holding two points. Twenty-one bytes of payload, wrapped in
 * the artifact's magic, sentinel and version — which is the whole
 * grammar, and which means this group runs on a fresh clone like every
 * other one here. Two points are far too thin to be a recording, and
 * that is deliberate: what the ladder does with a disk that holds
 * nothing to read is *refuse by name*, and a refusal is as much of the
 * surface as an answer.
 */

void group_flux(const char* directory)
{
    const std::string source = std::string{directory} + "/worked-example.remanence";
    if (!worked_example::place(source)) {
        failures += 1;
        std::printf("  FAIL cannot lay the artifact at %s\n", source.c_str());
        return;
    }

    // A missing artifact is a refusal like any other, and the image is
    // reached through its own type: there is no device to load one into.
    CHECK_REFUSES("opening an artifact that is not there",
                  [] { remanence::FluxImage::open("no-such-artifact-anywhere.remanence"); });

    remanence::FluxImage image = remanence::FluxImage::open(source);
    CHECK(image.format_id() == "remanence", "the artifact is not the remanence format");
    CHECK(image.path().has_value(), "an image opened by name answered no path");
    CHECK(image.access_mode() == remanence::AccessMode::ReadOnly,
          "a read open did not answer read-only");
    CHECK(image.form_factor() == "5.25-inch", "the form factor is not the one declared");
    CHECK(image.angular_divisions() == (1ull << 28),
          "the angular unit is not the format's own");
    CHECK(image.backing_bytes() > 0, "the decoded points are backed by nothing");
    CHECK(!image.provenance().empty(), "an image carries no account of what produced it");

    // The shape it reports: one hole, one surface, one orbit — exact
    // fractions, because nothing here is measured.
    const std::vector<remanence::FluxHole> holes = image.holes();
    CHECK(holes.size() == 1, "the worked example does not hold one hole");
    if (holes.size() == 1) {
        CHECK(holes[0].center_numerator == 3 && holes[0].center_denominator == 8,
              "the hole is not at three eighths of a turn");
        CHECK(holes[0].extent_numerator == 1 && holes[0].extent_denominator == 50,
              "the hole's extent is not one fiftieth");
    }
    CHECK(image.surfaces() == std::vector<std::uint64_t>{0}, "the image is not single-sided");

    const std::vector<remanence::FluxOrbit> orbits = image.orbits();
    CHECK(orbits.size() == 1, "the worked example does not hold one orbit");
    if (orbits.size() == 1) {
        CHECK(orbits[0].radius_microns == 57150, "the orbit is not at the declared radius");
        CHECK(orbits[0].points == 2, "the orbit does not hold two points");
        CHECK(orbits[0].coherent_points == 2, "a point lost its sense crossing the boundary");
    }

    // Writing it back: a new artifact, and an occupied destination
    // refused rather than overwritten.
    const std::string destination = std::string{directory} + "/round-trip.remanence";
    std::remove(destination.c_str());
    remanence::FluxWriteReport written = image.write(destination);
    CHECK(written.path().has_value(), "the write report names no artifact");
    CHECK(written.artifact_bytes() > 0, "the written artifact is empty");
    CHECK(written.orbits() == 1 && written.points() == 2,
          "the write did not carry the orbit it was given");
    CHECK_REFUSES("writing over an artifact that is already there",
                  [&] { image.write(destination); });

    remanence::FluxImage again = remanence::FluxImage::open(destination);
    CHECK(again.orbits().size() == image.orbits().size(),
          "the artifact did not reopen as what was written");

    // The ladder. Two points are not a recording, so what each rung
    // reports is the shape of nothing having been read — or a refusal
    // naming why, which is the answer this disk deserves.
    remanence::C1541Bitstream bits = image.materialize_c1541_bitstream();
    CHECK(!bits.profile_id().empty(), "the bitstream names no profile");
    CHECK(bits.cycles_per_rotation() > 0, "the profile declares no frame");
    CHECK(!bits.evidence().empty(), "the bitstream carries no evidence");
    for (const remanence::BitstreamLocation& location : bits.locations()) {
        CHECK(location.cell_cycles_denominator > 0, "a location has no cell");
    }

    remanence::C1541Bytestream bytes = bits.materialize_bytestream();
    CHECK(!bytes.codec_id().empty(), "the bytestream names no group code");
    CHECK(bytes.symbols_per_byte() > 0, "the codec resolves no byte at all");
    CHECK(!bytes.evidence().empty(), "the bytestream carries no evidence");

    // A track the stream does not hold is refused naming what it does.
    CHECK_REFUSES("asking for a track the stream does not hold",
                  [&] { bytes.location_bytes(99); });

    // **Nothing is manufactured to stand in for a recording.** Two
    // points frame no byte the record grammar names, so there is no
    // sector layer to hold and the refusal says exactly that rather
    // than handing back an empty one. The layer itself is walked over a
    // real capture in `flux_capture`, which needs a fixture.
    CHECK_REFUSES("recognizing sectors in a stream that frames no record",
                  [&] { bytes.recognize_sectors(); });

    // The renditions, each stating what it does not carry. A disk this
    // thin either renders empty or refuses by name; both are answers,
    // and what is checked is that the account comes back either way.
    try {
        remanence::D64Report d64 = image.describe_d64();
        CHECK(d64.blocks_defined() > 0, "a d64 rendition defines no blocks at all");
        CHECK(d64.blocks_read() == 0, "two points filled a d64 block");
        CHECK(d64.missing().size() == d64.blocks_defined(),
              "the d64 did not account for every block it defines");
    } catch (const remanence::Error&) {
        checks += 1; // refusing to render nothing is equally an answer
    }

    try {
        remanence::G64Report g64 = image.describe_g64();
        CHECK(!g64.half_tracks().empty() || !g64.declared_losses().empty(),
              "a g64 rendition carried neither a half-track nor an account of what it lost");
    } catch (const remanence::Error&) {
        checks += 1;
    }

    try {
        remanence::P64Report p64 = image.describe_p64();
        CHECK(p64.format_id() == "p64", "the p64 container is not the p64 format");
        CHECK(p64.reference_clock_hz() > 0, "the p64 declares no reference clock");
    } catch (const remanence::Error&) {
        checks += 1;
    }

    std::remove(source.c_str());
    std::remove(destination.c_str());
}

/* ------------------------------------------------------- a real capture
 *
 * The sector layer needs a recording that actually frames records, which
 * the worked example above deliberately is not. This walks the whole
 * ladder over a real KryoFlux capture: the archive, the capture set
 * gathered out of its namespace, the flux medium loaded from that, and
 * the bitstream, bytestream, sectors and namespace above it.
 *
 * It needs a fixture, so the Rust side gates it behind the `fixtures`
 * feature exactly as the core suite's flux tests are gated.
 */

/// This caller's own open, handed to the library with the format it is
/// declared to be — the library takes ownership of it (P7). The header
/// wraps no file API of its own, deliberately: whoever opens owns the
/// lock, and that open is the caller's.
remanence::NativeHandle open_source(const char* path)
{
#ifdef _WIN32
    HANDLE handle = CreateFileA(path, GENERIC_READ, FILE_SHARE_READ, nullptr, OPEN_EXISTING,
                                FILE_ATTRIBUTE_NORMAL, nullptr);
    return handle == INVALID_HANDLE_VALUE ? 0 : reinterpret_cast<remanence::NativeHandle>(handle);
#else
    const int descriptor = open(path, O_RDONLY);
    return descriptor < 0 ? 0 : static_cast<remanence::NativeHandle>(descriptor);
#endif
}

void group_flux_capture(const char* archive_path)
{
    remanence::Session session;

    const remanence::NativeHandle source = open_source(archive_path);
    if (source == 0) {
        failures += 1;
        std::printf("  FAIL cannot open the capture archive at %s\n", archive_path);
        return;
    }

    // The archive is a medium like any other; its namespace is its
    // native vantage.
    remanence::Medium archive = session.load_media(source, "7z");
    checks += 1;

    std::optional<remanence::Partition> whole = archive.partition(0);
    CHECK(whole.has_value(), "an archive medium bears no direct partition");
    if (!whole.has_value()) {
        return;
    }

    // The capture set: every member gathered in one pass, free-standing
    // and riding the archive's own claim.
    remanence::Filesystem namespace_ = whole->filesystem();
    remanence::FileSourceList members = namespace_.files();
    CHECK(members.size() > 1, "a capture set is more than one stream");

    // One disk, spread over a stream per head per step position. The
    // format records one device type, so no declaration is needed.
    remanence::Medium disk = session.load_media_sources(std::move(members), "kryoflux");
    CHECK(disk.device_type().has_value(), "a capture recorded by nothing");
    CHECK(!disk.assurance().evidence().empty(),
          "the reduction carried no account of what it did");

    // The bitstream, from the pooled medium: materialized once, into the
    // medium itself, and answered from then on.
    remanence::C1541Bitstream bits = disk.bitstream();
    CHECK(!bits.locations().empty(), "a real capture resolved no location at all");
    CHECK(bits.cycles_per_rotation() > 0, "the profile declares no frame");

    remanence::C1541Bytestream bytes = disk.bytestream();
    CHECK(!bytes.locations().empty(), "the bytestream framed nothing");
    const std::uint64_t held = bytes.location_bytes(1);
    CHECK(held > 0, "track one framed no bytes");
    const std::vector<std::uint8_t> first = bytes.location_read_at(1, 0, 1);
    CHECK(first.size() == 1, "the first framed byte did not read");

    // The sectors the recording states for itself.
    remanence::C1541Sectors sectors = bytes.recognize_sectors();
    CHECK(sectors.payload_bytes() > 0, "the grammar declares no payload size");
    CHECK(!sectors.grammar_id().empty(), "the sector layer names no grammar");
    CHECK(sectors.claim_count() > 0, "a real recording stated no record at all");
    CHECK(!sectors.evidence().empty(), "the sector layer carries no evidence");

    bool readable = false;
    for (std::size_t at = 0; at < sectors.claim_count(); at += 1) {
        const remanence::SectorClaim claim = sectors.claim(at);
        if (claim.readable) {
            readable = true;
            // A claim that reads broke no rule, which the layer spells
            // as an empty rule rather than a rule named "none".
            CHECK(sectors.claim_rule(at).empty(), "a readable claim named a broken rule");
            CHECK(sectors.claim_refusal(at).empty(), "a readable claim carried a refusal");
            break;
        }
    }
    CHECK(readable, "no claim of a real recording reads");

    // The BAM, by the address the recording states for it.
    const std::vector<std::uint8_t> bam = sectors.read(18, 0);
    CHECK(bam.size() == sectors.payload_bytes(), "the BAM is not one payload long");
    CHECK(bam.size() > 2 && bam[0] == 18, "the BAM does not open at its own directory track");

    // The directory CBM DOS wrote across those sectors, through the
    // recording's own direct partition.
    std::optional<remanence::Partition> direct = sectors.partition();
    CHECK(direct.has_value(), "the sector layer bears no direct partition");
    if (direct.has_value()) {
        CHECK(direct->is_direct(), "the composed partition is not the direct one");
        remanence::Filesystem catalog = direct->filesystem_as("cbmdos");
        CHECK(catalog.has_namespace(), "the declared reading composed no namespace");
        // The listing is bound to a name because an Entry borrows it —
        // `catalog.entries().entries()` is deleted for exactly that
        // reason and does not compile.
        remanence::EntryList listed = catalog.entries();
        CHECK(!listed.empty(), "the catalog lists nothing at all");
        for (const remanence::Entry& entry : listed.entries()) {
            CHECK(!entry.name().empty(), "a catalog entry has no name");
        }
    }
}

/* ------------------------------------------------- the families are disjoint
 *
 * A block medium's recording is presented by its format adapter and a
 * flux medium's by the ladder above; the two do not meet (P13). What
 * that has to mean at this surface is that the flux doors on a block
 * medium refuse by name rather than answering something empty.
 */
void group_flux_refusals()
{
    remanence::Session session;
    remanence::Medium blank = blank_disk(session);

    CHECK_REFUSES("asking a block medium for a bitstream",
                  [&]() mutable { blank.bitstream(); });
    CHECK_REFUSES("asking a block medium for a bytestream",
                  [&]() mutable { blank.bytestream(); });
}

/* ---------------------------------------------------- a real artifact
 *
 * The one group that needs an image, because the report, the composed
 * volumes and the namespace above them are answers only a recording
 * has. It is the whole journey a consumer makes: claim by name, load
 * over the claim already held, inspect, and read a file out.
 */
void group_report(const char* path)
{
    remanence::Session session;
    remanence::Discovery discovery = remanence::discover_media(path);

    CHECK(discovery.image_format().has_value(), "a discovery answered no image format");
    CHECK(discovery.article().has_value(), "a discovery answered no article");
    CHECK(discovery.size() > 0, "a discovery answered a zero size");
    CHECK(discovery.mode() == remanence::AccessMode::ReadOnly,
          "a read-intent discovery did not answer read-only");
    CHECK(discovery.assurance().outcome() == remanence::AssuranceOutcome::Verified,
          "a sound artifact did not verify");
    CHECK(discovery.assurance().claim() == remanence::Claim::LibraryOpened,
          "an artifact reached by name is not the library's own open");

    // A format recording several device types leaves the choice to the
    // caller, and the discovery says which it is by answering or not.
    const std::vector<std::string> recorded = discovery.recorded_devices();
    const std::optional<std::string> declared = discovery.device_type();
    CHECK(!recorded.empty(), "the recognizing format records no device type at all");

    remanence::Medium medium =
        declared.has_value()
            ? session.load_discovery(std::move(discovery))
            : session.load_discovery_as(std::move(discovery), recorded.front());

    CHECK(session.media_count() == 1, "the load did not pool the medium");
    CHECK(medium.size() > 0, "a loaded medium has no extent");
    CHECK(medium.mode() == remanence::AccessMode::ReadOnly,
          "a read-intent load did not stay read-only");

    // Identification, layer by layer, with its evidence (P4).
    remanence::Identification identification = medium.identify();
    CHECK(identification.layer_count() > 0, "a real artifact identified no layer");
    CHECK(!identification.evidence().empty(), "an identification carried no evidence");
    for (const remanence::Layer& layer : identification.layers()) {
        CHECK(layer.id().has_value(), "a recognized layer has no id");
        CHECK(layer.confidence() <= 100, "a confidence above 100");
    }

    // The coordinates the artifact stated for itself, and who stated
    // them.
    remanence::Geometry geometry = medium.geometry();
    for (const remanence::GeometryReading& reading : geometry.readings()) {
        CHECK(!reading.source().empty(), "a geometry reading names no source");
    }

    // The layered report.
    remanence::DiskReport report = medium.inspect();
    CHECK(report.device_length_bytes() == medium.size(),
          "the report's extent is not the medium's");
    CHECK(report.content() == remanence::DiskContent::Schema,
          "a partitioned artifact was not reported as bearing a schema");
    CHECK(report.has_partition_schema(), "a partitioned artifact reported no schema");
    CHECK(report.partition_schema_kind().has_value(), "the schema has no kind");
    CHECK(report.region_count() > 0, "a schema declared no region at all");
    for (const remanence::ReportRegion& region : report.regions()) {
        CHECK(region.length_bytes() > 0, "a declared region is empty");
    }
    CHECK(report.volume_count() > 0, "nothing composed over the declared regions");
    for (const remanence::ReportVolume& volume : report.volumes()) {
        CHECK(volume.length_bytes() > 0, "a composed volume is empty");
    }
    for (const remanence::ReportFilesystem& filesystem : report.filesystems()) {
        CHECK(filesystem.kind().has_value(), "a recognized filesystem has no kind");
        // The label readings are the sources consulted, in policy order.
        for (const remanence::LabelReading& reading : filesystem.label_readings()) {
            CHECK(!reading.source.empty(), "a label reading names no source");
        }
    }

    // The pool of partitions, and the namespace above one of them.
    CHECK(medium.partition_scheme().has_value(), "a partitioned medium recorded no scheme");
    CHECK(medium.partition_count() > 0, "the evidence pool holds no partition");

    bool walked = false;
    for (std::uint32_t ordinal : medium.partition_ordinals()) {
        std::optional<remanence::Partition> partition = medium.partition(ordinal);
        CHECK(partition.has_value(), "an ordinal the pool listed answered no partition");
        if (!partition.has_value() || !partition->bears_namespace()) {
            continue;
        }

        remanence::Filesystem filesystem = partition->filesystem();
        CHECK(filesystem.has_namespace(), "a namespace-bearing partition composed none");
        CHECK(filesystem.kind().has_value(), "a recognized filesystem has no kind");
        CHECK(!filesystem.evidence().empty(), "a recognized namespace carried no evidence");

        remanence::EntryList root = filesystem.entries();
        for (const remanence::Entry& entry : root.entries()) {
            CHECK(!entry.name().empty(), "a listed entry has no name");
            if (entry.kind() != remanence::EntryKind::File || entry.size_bytes() == 0) {
                continue;
            }
            // One file read two ways: the whole-value door, and the
            // bounded one that streams.
            remanence::File file = filesystem.get_file(std::string{entry.name()});
            remanence::FileData whole = file.bytes();
            CHECK(whole.size() == entry.size_bytes(),
                  "a file read whole is not the size the listing claimed");
            const std::vector<std::uint8_t> head = file.read_at(0, 1);
            CHECK(!whole.empty() && head.size() == 1 && head[0] == whole.data()[0],
                  "the bounded read disagrees with the whole one");
            walked = true;
            break;
        }
        if (walked) {
            break;
        }
    }
    CHECK(walked, "no readable file was found to walk on the artifact");

    // Absence is an answer here, and failure is not: nothing there is an
    // empty listing, while asking for it as a file is refused.
    for (std::uint32_t ordinal : medium.partition_ordinals()) {
        std::optional<remanence::Partition> partition = medium.partition(ordinal);
        if (!partition.has_value() || !partition->bears_namespace()) {
            continue;
        }
        remanence::Filesystem filesystem = partition->filesystem();
        CHECK(filesystem.stat("NO-SUCH.FIL").empty(),
              "a path that is not there did not answer an empty listing");
        CHECK_REFUSES("asking for a file that is not there",
                      [&] { filesystem.get_file("NO-SUCH.FIL"); });
        break;
    }

    session.release_media(medium.id());
    CHECK(session.media_count() == 0, "release left the medium pooled");
}

} // namespace

int main(int argc, char** argv)
{
    if (argc < 2) {
        std::printf("usage: wrapper <group>\n");
        return 2;
    }

    const std::string group{argv[1]};
    try {
        if (group == "catalogs") {
            group_catalogs();
        } else if (group == "refusals") {
            group_refusals();
        } else if (group == "authorship") {
            group_authorship();
        } else if (group == "views") {
            group_views();
        } else if (group == "lifetimes") {
            group_lifetimes();
        } else if (group == "absences") {
            group_absences();
        } else if (group == "flux_refusals") {
            group_flux_refusals();
        } else if (group == "flux_capture") {
            if (argc < 3) {
                std::printf("the flux_capture group needs an artifact path\n");
                return 2;
            }
            group_flux_capture(argv[2]);
        } else if (group == "flux") {
            if (argc < 3) {
                std::printf("the flux group needs a scratch directory\n");
                return 2;
            }
            group_flux(argv[2]);
        } else if (group == "report") {
            if (argc < 3) {
                std::printf("the report group needs an artifact path\n");
                return 2;
            }
            group_report(argv[2]);
        } else {
            std::printf("no such group: %s\n", group.c_str());
            return 2;
        }
    } catch (const remanence::Error& refusal) {
        std::printf("  FAIL the group did not finish: %s\n", refusal.what());
        return 1;
    } catch (const std::exception& other) {
        std::printf("  FAIL the group threw: %s\n", other.what());
        return 1;
    }

    std::printf("  %d checks, %d failures\n", checks, failures);
    return failures == 0 ? 0 : 1;
}
