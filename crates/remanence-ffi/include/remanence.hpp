/*
 * SPDX-FileCopyrightText: 2026 Paul Galbraith
 * SPDX-License-Identifier: GPL-3.0-only
 */

#ifndef REMANENCE_HPP
#define REMANENCE_HPP

/*
 * An idiomatic C++ presentation of the C ABI (S2).
 *
 * **This is a derived representation, and S2 remains the norm.** The C
 * header is a derived representation of the Rust `extern "C"` items;
 * this is a derived representation of that header, one layer further
 * out. It is header-only, it links nothing of its own, and every line
 * of it is a call to a `remanence_*` function you could have made
 * yourself — so it claims no capability the C ABI does not already
 * provide, and it is no fourth application surface. What it adds is
 * ergonomics: lifetimes that end themselves, refusals that cannot be
 * ignored, enumerations that do not decay to `int`, and strings whose
 * ownership is in their type.
 *
 * It moves with S2 in the same change, never independently. Nothing
 * generates it — the C header regenerates from the Rust on every build,
 * and this is written by hand beside it — so a `remanence_*` function
 * added, renamed or retired moves this file in that same change, and a
 * disagreement between the two is a defect here, the ABI being the norm.
 *
 * **It covers the whole ABI.** Every node of the storage model is here
 * — the session with its devices and its media,
 * discoveries, partitions, volumes, filesystems and files — with the
 * records they hand back: assurance, geometry, identification,
 * inspection reports, directory listings, file bytes and load sources.
 * So is the flux ladder beside it: the
 * remanence image, the family's hardware bitstream, the encoded
 * bytestream, the recognized sectors, and the d64, g64 and p64
 * renditions. Where a `remanence_*` function is not wrapped, that is a
 * defect rather than a boundary — and `<remanence.h>` is included here,
 * so the C function is one call away either way.
 *
 * **Refusals are exceptions.** Every fallible call throws
 * `remanence::Error`, carrying the delivered `ErrorCategory` — the
 * stable classification that says how to behave — the human diagnostic,
 * and the rule identity where the refusal broke one of an enumerated set
 * (P10). Nothing here returns a status code a caller can drop on the
 * floor. Where the ABI distinguishes an *absence* from a *failure* —
 * a filesystem with no label field, a lookup of a device that is not
 * there — the absence comes back as an empty `std::optional` and only
 * the failure throws.
 *
 * **Ownership follows the ABI's own division, and so do the classes.**
 * A handle the ABI gives you to free — a discovery, a partition, a
 * space, a file, a report — is held by a move-only RAII class whose
 * destructor calls that handle's `_free`. A handle the session owns and
 * documents as "never free it" — a device, a medium — is a
 * copyable **view**, because there is no ownership for a destructor to
 * discharge. The distinction is the ABI's, not this header's invention.
 *
 * **View lifetimes are documented, not enforced.** A `StorageDevice`
 * and a `Medium` stay valid until the session releases
 * them or dies; a `Layer`, an `Entry`, a `ReportRegion` and their kin
 * are borrowed from the handle that answered them and stay valid until
 * it dies. C++ cannot enforce those bounds in general and this header
 * does not pretend to — outliving the handle is undefined exactly as it
 * is in C. The one case it *can* enforce, it does: an accessor that
 * hands back a borrowed record is deleted on an rvalue, so
 * `medium.identify().layers()` fails to compile rather than answering
 * views of a handle that died at the semicolon. Bind the handle to a
 * name and the same line compiles.
 *
 * **Strings are the one place that discipline is bought out rather than
 * documented.** Every accessor on a handle answers a `std::string` you
 * own, copied on the way out, because the alternative — a view into the
 * handle's memory — dangles the moment anyone writes
 * `discover_media(path).article()`, which is precisely the expression
 * RAII invites. The catalogs are the exception and answer
 * `std::string_view`: those strings are static for the life of the
 * release, so there is nothing for them to outlive. Where a caller wants
 * the zero-copy pointer instead, `get()` hands over the raw handle and
 * the C accessor is one call away.
 *
 * Requires C++17. The library itself is C, which is where the boundary
 * stays: C++ has no stable cross-compiler ABI, so no compiled C++
 * artifact exists or will.
 *
 *   #include <remanence.hpp>
 *
 *   remanence::Session session;
 *   remanence::Discovery found = remanence::discover_media("disk.h8d");
 *   remanence::Medium medium = session.load_discovery(std::move(found));
 *
 *   remanence::Identification what = medium.identify();
 *   for (const remanence::Layer& layer : what.layers()) {
 *       std::cout << layer.id().value_or("?") << ' '
 *                 << unsigned{layer.confidence()} << '\n';
 *   }
 */

#include <remanence.h>

#include <cstddef>
#include <cstdint>
#include <new>
#include <optional>
#include <stdexcept>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

namespace remanence {

// ---------------------------------------------------------------------
// The enumerated sets
//
// One scoped enumeration per C enum, each constant defined *as* the C
// constant, so the mapping is exact by construction rather than by a
// table that could drift. The underlying values are the ABI's own, which
// is what makes `static_cast` between them well defined in both
// directions.
// ---------------------------------------------------------------------

/// Stable, machine-readable classification of a library refusal (P10).
enum class ErrorCategory : std::int32_t {
    Locked = REMANENCE_ERROR_CATEGORY_LOCKED,
    InvalidImage = REMANENCE_ERROR_CATEGORY_INVALID_IMAGE,
    Unsupported = REMANENCE_ERROR_CATEGORY_UNSUPPORTED,
    ReadOnly = REMANENCE_ERROR_CATEGORY_READ_ONLY,
    NotFound = REMANENCE_ERROR_CATEGORY_NOT_FOUND,
    NotDirectory = REMANENCE_ERROR_CATEGORY_NOT_DIRECTORY,
    IsDirectory = REMANENCE_ERROR_CATEGORY_IS_DIRECTORY,
    NoSpace = REMANENCE_ERROR_CATEGORY_NO_SPACE,
    Unavailable = REMANENCE_ERROR_CATEGORY_UNAVAILABLE,
    Io = REMANENCE_ERROR_CATEGORY_IO,
};

/// The caller's declared intent when opening a disk (P7).
enum class AccessIntent : std::int32_t {
    Read = REMANENCE_ACCESS_INTENT_READ,
    Write = REMANENCE_ACCESS_INTENT_WRITE,
};

/// A medium's effective access mode (P7, P28).
enum class AccessMode : std::int32_t {
    ReadWrite = REMANENCE_ACCESS_MODE_READ_WRITE,
    ReadOnly = REMANENCE_ACCESS_MODE_READ_ONLY,
};

/// The image container format a disk image turned out to be.
enum class DiskFormat : std::int32_t {
    Raw = REMANENCE_DISK_FORMAT_RAW,
    Qcow2 = REMANENCE_DISK_FORMAT_QCOW2,
    Vdi = REMANENCE_DISK_FORMAT_VDI,
};

/// What a recognized layer of an artifact's nesting is.
enum class LayerKind : std::int32_t {
    Archive = REMANENCE_LAYER_KIND_ARCHIVE,
    Image = REMANENCE_LAYER_KIND_IMAGE,
    PhysicalMedia = REMANENCE_LAYER_KIND_PHYSICAL_MEDIA,
    Filesystem = REMANENCE_LAYER_KIND_FILESYSTEM,
    Unknown = REMANENCE_LAYER_KIND_UNKNOWN,
};

/// Which layout details a layer carries.
enum class LayoutKind : std::int32_t {
    Unknown = REMANENCE_LAYOUT_KIND_UNKNOWN,
    Archive = REMANENCE_LAYOUT_KIND_ARCHIVE,
    Image = REMANENCE_LAYOUT_KIND_IMAGE,
    PhysicalMedia = REMANENCE_LAYOUT_KIND_PHYSICAL_MEDIA,
    Filesystem = REMANENCE_LAYOUT_KIND_FILESYSTEM,
};

/// Sector arrangement across a disk.
enum class SectorLayoutKind : std::int32_t {
    Unknown = REMANENCE_SECTOR_LAYOUT_KIND_UNKNOWN,
    Fixed = REMANENCE_SECTOR_LAYOUT_KIND_FIXED,
    Variable = REMANENCE_SECTOR_LAYOUT_KIND_VARIABLE,
};

/// Whose open a medium's P7 claim is.
enum class Claim : std::int32_t {
    LibraryOpened = REMANENCE_CLAIM_LIBRARY_OPENED,
    CallerOpened = REMANENCE_CLAIM_CALLER_OPENED,
    Authored = REMANENCE_CLAIM_AUTHORED,
};

/// What an open established about the evidence beneath it (P28).
enum class AssuranceOutcome : std::int32_t {
    Verified = REMANENCE_ASSURANCE_OUTCOME_VERIFIED,
    Degraded = REMANENCE_ASSURANCE_OUTCOME_DEGRADED,
    Refused = REMANENCE_ASSURANCE_OUTCOME_REFUSED,
};

/// What the evidence established about a medium's geometry.
enum class GeometryState : std::int32_t {
    Unstated = REMANENCE_GEOMETRY_STATE_UNSTATED,
    Determined = REMANENCE_GEOMETRY_STATE_DETERMINED,
    Undetermined = REMANENCE_GEOMETRY_STATE_UNDETERMINED,
};

/// How a schema declares a region: data, or structure.
enum class RegionRole : std::int32_t {
    Data = REMANENCE_REGION_ROLE_DATA,
    Structure = REMANENCE_REGION_ROLE_STRUCTURE,
};

/// What a directory entry is.
enum class EntryKind : std::int32_t {
    File = REMANENCE_ENTRY_KIND_FILE,
    Directory = REMANENCE_ENTRY_KIND_DIRECTORY,
};

/// What the device's leading structure turned out to be.
enum class DiskContent : std::int32_t {
    Blank = REMANENCE_DISK_CONTENT_BLANK,
    Schema = REMANENCE_DISK_CONTENT_SCHEMA,
    DirectVolume = REMANENCE_DISK_CONTENT_DIRECT_VOLUME,
    UnknownNonblank = REMANENCE_DISK_CONTENT_UNKNOWN_NONBLANK,
};

/// Where a volume's storage came from.
enum class VolumeOrigin : std::int32_t {
    WholeDevice = REMANENCE_VOLUME_ORIGIN_WHOLE_DEVICE,
    Regions = REMANENCE_VOLUME_ORIGIN_REGIONS,
};

/// The caller's own open file, as the ABI takes it: a Windows `HANDLE`
/// or a POSIX file descriptor, widened to the ABI's `ptrdiff_t`. The
/// library takes ownership of what is passed (P7).
using NativeHandle = std::ptrdiff_t;

// ---------------------------------------------------------------------
// Refusals
// ---------------------------------------------------------------------

/// A library refusal, thrown by every fallible verb.
///
/// The `what()` text is the human diagnostic (P6) and no release
/// promises to keep its wording. `category()` is the stable
/// classification an embedder maps behaviour from, and `rule()` names
/// which rule of an enumerated set the input broke where the refusal
/// belongs to one — absent where it does not, which is ordinary rather
/// than an omission (P10).
class Error : public std::runtime_error {
public:
    Error(ErrorCategory category, const std::string& message, std::optional<std::string> rule)
        : std::runtime_error(message), category_(category), rule_(std::move(rule))
    {
    }

    /// How the caller should behave. Always present.
    ErrorCategory category() const noexcept { return category_; }

    /// Which rule was broken, where an enumerated set owns one.
    const std::optional<std::string>& rule() const noexcept { return rule_; }

private:
    ErrorCategory category_;
    std::optional<std::string> rule_;
};

// ---------------------------------------------------------------------
// detail — the machinery every wrapper below is built out of
// ---------------------------------------------------------------------

namespace detail {

/// A string the library owns for the whole life of the release — a
/// catalog's — as a view. Null becomes empty.
inline std::string_view text(const char* value) noexcept
{
    return value == nullptr ? std::string_view{} : std::string_view{value};
}

/// The same where null is an answer rather than an absence of one.
inline std::optional<std::string_view> optional_text(const char* value) noexcept
{
    if (value == nullptr) {
        return std::nullopt;
    }
    return std::string_view{value};
}

/// A string a handle owns, copied out. **Every accessor on a handle
/// copies**, which is the whole of why this header has no dangling
/// answer to give: the borrowed pointer dies with the handle, and a
/// temporary handle is exactly what an RAII wrapper invites a caller to
/// write.
inline std::string copied(const char* value)
{
    return value == nullptr ? std::string{} : std::string{value};
}

/// The same where null is an answer rather than an absence of one.
inline std::optional<std::string> optional_copied(const char* value)
{
    if (value == nullptr) {
        return std::nullopt;
    }
    return std::string{value};
}

/// Frees an allocated string however this scope leaves.
class OwnedString {
public:
    explicit OwnedString(char* value) noexcept : value_(value) {}
    ~OwnedString() { remanence_string_free(value_); }
    OwnedString(const OwnedString&) = delete;
    OwnedString& operator=(const OwnedString&) = delete;

    const char* get() const noexcept { return value_; }

private:
    char* value_;
};

/// An allocated string, copied out and given straight back.
inline std::optional<std::string> owned_text(char* value)
{
    OwnedString held(value);
    if (value == nullptr) {
        return std::nullopt;
    }
    return std::string{value};
}

/// A NUL-terminated pointer for an optional input string.
inline const char* pointer(const std::optional<std::string>& value) noexcept
{
    return value.has_value() ? value->c_str() : nullptr;
}

/// The three out-parameters a fallible call writes, freed however the
/// call turns out, and turned into an `Error` where one was written.
///
/// The category is seeded with `Io` rather than left indeterminate: a
/// library that returned failure without writing one would otherwise
/// have this header read uninitialised memory to build the exception.
class Outcome {
public:
    Outcome() = default;
    ~Outcome()
    {
        remanence_string_free(message_);
        remanence_string_free(rule_);
    }
    Outcome(const Outcome&) = delete;
    Outcome& operator=(const Outcome&) = delete;

    RemanenceErrorCategory* category() noexcept { return &category_; }
    char** message() noexcept { return &message_; }
    char** rule() noexcept { return &rule_; }

    /// Whether the library wrote a refusal, which is how a call that
    /// answers null for an honest absence is told from one that failed.
    bool refused() const noexcept { return message_ != nullptr; }

    /// Throws the refusal the library wrote, or one saying `what` where
    /// it wrote none.
    [[noreturn]] void raise(const char* what) const
    {
        std::optional<std::string> rule;
        if (rule_ != nullptr) {
            rule = std::string{rule_};
        }
        throw Error(static_cast<ErrorCategory>(category_),
                    message_ != nullptr ? std::string{message_} : std::string{what},
                    std::move(rule));
    }

    /// Passes a successful call's answer through; throws otherwise.
    void require(bool answered, const char* what) const
    {
        if (!answered) {
            raise(what);
        }
    }

    template <typename T>
    T* require(T* answered, const char* what) const
    {
        if (answered == nullptr) {
            raise(what);
        }
        return answered;
    }

private:
    RemanenceErrorCategory category_ = REMANENCE_ERROR_CATEGORY_IO;
    char* message_ = nullptr;
    char* rule_ = nullptr;
};

/// How one owned handle type is released. Specialised for each; there is
/// no primary definition, so a handle nobody taught this header to free
/// fails to compile rather than leaking.
template <typename T>
struct Release;

template <>
struct Release<RemanenceSession> {
    void operator()(RemanenceSession* handle) const noexcept { remanence_session_free(handle); }
};
template <>
struct Release<RemanenceDiscovery> {
    void operator()(RemanenceDiscovery* handle) const noexcept { remanence_discovery_free(handle); }
};
template <>
struct Release<RemanenceIdentification> {
    void operator()(RemanenceIdentification* handle) const noexcept
    {
        remanence_identification_free(handle);
    }
};
template <>
struct Release<RemanenceAssurance> {
    void operator()(RemanenceAssurance* handle) const noexcept { remanence_assurance_free(handle); }
};
template <>
struct Release<RemanenceGeometry> {
    void operator()(RemanenceGeometry* handle) const noexcept { remanence_geometry_free(handle); }
};
template <>
struct Release<RemanencePartition> {
    void operator()(RemanencePartition* handle) const noexcept { remanence_partition_free(handle); }
};
template <>
struct Release<RemanenceSpace> {
    void operator()(RemanenceSpace* handle) const noexcept { remanence_space_free(handle); }
};
template <>
struct Release<RemanenceFile> {
    void operator()(RemanenceFile* handle) const noexcept { remanence_file_free(handle); }
};
template <>
struct Release<RemanenceFileData> {
    void operator()(RemanenceFileData* handle) const noexcept { remanence_file_data_free(handle); }
};
template <>
struct Release<RemanenceFileSource> {
    void operator()(RemanenceFileSource* handle) const noexcept
    {
        remanence_file_source_free(handle);
    }
};
template <>
struct Release<RemanenceFileSourceList> {
    void operator()(RemanenceFileSourceList* handle) const noexcept
    {
        remanence_file_source_list_free(handle);
    }
};
template <>
struct Release<RemanenceEntryList> {
    void operator()(RemanenceEntryList* handle) const noexcept { remanence_entry_list_free(handle); }
};
template <>
struct Release<RemanenceDiskReport> {
    void operator()(RemanenceDiskReport* handle) const noexcept { remanence_report_free(handle); }
};
template <>
struct Release<RemanenceFluxImage> {
    void operator()(RemanenceFluxImage* handle) const noexcept { remanence_flux_image_free(handle); }
};
template <>
struct Release<RemanenceFluxWriteReport> {
    void operator()(RemanenceFluxWriteReport* handle) const noexcept
    {
        remanence_flux_write_report_free(handle);
    }
};
template <>
struct Release<RemanenceC1541Bitstream> {
    void operator()(RemanenceC1541Bitstream* handle) const noexcept
    {
        remanence_c1541_bitstream_free(handle);
    }
};
template <>
struct Release<RemanenceC1541Bytestream> {
    void operator()(RemanenceC1541Bytestream* handle) const noexcept
    {
        remanence_c1541_bytestream_free(handle);
    }
};
template <>
struct Release<RemanenceC1541Sectors> {
    void operator()(RemanenceC1541Sectors* handle) const noexcept
    {
        remanence_c1541_sectors_free(handle);
    }
};
template <>
struct Release<RemanenceD64Report> {
    void operator()(RemanenceD64Report* handle) const noexcept
    {
        remanence_d64_report_free(handle);
    }
};
template <>
struct Release<RemanenceG64Report> {
    void operator()(RemanenceG64Report* handle) const noexcept
    {
        remanence_g64_report_free(handle);
    }
};
template <>
struct Release<RemanenceP64Report> {
    void operator()(RemanenceP64Report* handle) const noexcept
    {
        remanence_p64_report_free(handle);
    }
};

/// One owned handle's lifetime. Move-only, because the ABI's handles are
/// not copyable: two owners would free one allocation twice.
///
/// Freeing null is defined for every owned handle the ABI hands out, so
/// a moved-from or default-constructed holder needs no guard.
template <typename T>
class Owned {
public:
    Owned() noexcept = default;
    explicit Owned(T* handle) noexcept : handle_(handle) {}
    ~Owned() { Release<T>{}(handle_); }

    Owned(Owned&& other) noexcept : handle_(other.handle_) { other.handle_ = nullptr; }
    Owned& operator=(Owned&& other) noexcept
    {
        if (this != &other) {
            Release<T>{}(handle_);
            handle_ = other.handle_;
            other.handle_ = nullptr;
        }
        return *this;
    }
    Owned(const Owned&) = delete;
    Owned& operator=(const Owned&) = delete;

    T* get() const noexcept { return handle_; }

    /// Gives up ownership — for the verbs that consume a handle.
    T* release() noexcept
    {
        T* handle = handle_;
        handle_ = nullptr;
        return handle;
    }

private:
    T* handle_ = nullptr;
};

/// What every owned wrapper below inherits: the handle, and the three
/// things a caller does with a raw one.
///
/// The destructor is protected and non-virtual — these are values, not a
/// hierarchy, and nothing is ever deleted through this type.
template <typename T>
class Held {
public:
    explicit Held(T* adopted) noexcept : handle_(adopted) {}

    /// The raw handle, for the C functions this header does not wrap.
    /// Borrowed: the wrapper still frees it.
    T* get() const noexcept { return handle_.get(); }

    /// Gives up ownership, for a C caller that will free it instead.
    T* release() noexcept { return handle_.release(); }

    explicit operator bool() const noexcept { return get() != nullptr; }

protected:
    ~Held() = default;
    Held(Held&&) noexcept = default;
    Held& operator=(Held&&) noexcept = default;

    Owned<T> handle_;
};

/// Checks an index against a count the ABI answers, so an out-of-range
/// read is a C++ exception rather than the ABI's null-or-zero answer.
inline void in_range(std::size_t index, std::size_t count, const char* what)
{
    if (index >= count) {
        throw std::out_of_range(what);
    }
}

} // namespace detail

// ---------------------------------------------------------------------
// The library itself, and the catalogs it claims
//
// These are pure lookups over what this release enumerates (P3): no
// artifact, no session, no I/O. Every string in them is owned by the
// library and outlives any call, so the views are safe to keep.
// ---------------------------------------------------------------------

/// The library version.
inline std::string_view version() noexcept
{
    return detail::text(remanence_version());
}

/// The stated default session cache bound, in bytes (P27).
inline std::uint64_t default_cache_bytes() noexcept
{
    return remanence_default_cache_bytes();
}

/// One format a load may declare.
struct Format {
    /// The stable spelling — `qcow2`, `7z` — a load names it by.
    std::string_view id;
    std::string_view name;
    /// The device types this format records: one where it carries the
    /// type bare, several where the load declares which, none for an
    /// archive grammar.
    std::vector<std::string_view> device_types;
    /// Whether a declaration of it carries the block size (the raw
    /// reading alone).
    bool takes_block_bytes;
    /// Whether it reads a collection of sources rather than one artifact.
    bool takes_collection;
};

inline std::vector<Format> formats()
{
    std::vector<Format> claimed;
    const std::size_t count = remanence_format_count();
    claimed.reserve(count);
    for (std::size_t at = 0; at < count; at += 1) {
        Format format;
        format.id = detail::text(remanence_format_id(at));
        format.name = detail::text(remanence_format_name(at));
        format.takes_block_bytes = remanence_format_takes_block_bytes(at);
        format.takes_collection = remanence_format_takes_collection(at);
        const std::size_t devices = remanence_format_device_count(at);
        format.device_types.reserve(devices);
        for (std::size_t device = 0; device < devices; device += 1) {
            format.device_types.push_back(detail::text(remanence_format_device(at, device)));
        }
        claimed.push_back(std::move(format));
    }
    return claimed;
}

/// One slot a device may be added at: a device type from the catalog
/// (P14), or the archive receiver, which records nothing.
struct DeviceSlot {
    /// The stable spelling — `c1541`, `mbr-block-hd`, `archive`.
    std::string_view id;
    std::string_view name;
    /// Where the device-type declaration came from; absent for the
    /// archive receiver.
    std::optional<std::string_view> provenance;
    /// `floppy`, `hard-drive` or `optical`; absent for the archive
    /// receiver.
    std::optional<std::string_view> device_class;
    /// The article this device type is served; absent as above.
    std::optional<std::string_view> article;
    /// The bay half of every attachment identity here — `hdd` for `hdd0`.
    std::string_view prefix;
    /// The drive profile it claims as its recording path (P22), where it
    /// claims one.
    std::optional<std::string_view> flux_path;
    /// The partition scheme it lays content out under, where it declares
    /// one.
    std::optional<std::string_view> scheme;
    /// `sector` or `block`; absent for the archive receiver, which is no
    /// device type at all.
    std::optional<std::string_view> addressing;
};

inline std::vector<DeviceSlot> device_slots()
{
    std::vector<DeviceSlot> slots;
    const std::size_t count = remanence_device_slot_count();
    slots.reserve(count);
    for (std::size_t at = 0; at < count; at += 1) {
        DeviceSlot slot;
        slot.id = detail::text(remanence_device_slot_id(at));
        slot.name = detail::text(remanence_device_slot_name(at));
        slot.provenance = detail::optional_text(remanence_device_slot_provenance(at));
        slot.device_class = detail::optional_text(remanence_device_slot_class(at));
        slot.article = detail::optional_text(remanence_device_slot_article(at));
        slot.prefix = detail::text(remanence_device_slot_prefix(at));
        slot.flux_path = detail::optional_text(remanence_device_slot_flux_path(at));
        slot.scheme = detail::optional_text(remanence_device_slot_scheme(at));
        slot.addressing = detail::optional_text(remanence_device_slot_addressing(at));
        slots.push_back(slot);
    }
    return slots;
}

/// One kind of blank medium this release authors.
struct NewMediaKind {
    /// The stable spelling — `chs-disk`, `flexible-5.25-soft`.
    std::string_view id;
    std::string_view name;
    /// The article a medium of this kind is.
    std::string_view article;
    /// Whether its declaration carries the recording's coordinates.
    bool takes_geometry;
};

inline std::vector<NewMediaKind> new_media_kinds()
{
    std::vector<NewMediaKind> kinds;
    const std::size_t count = remanence_new_media_count();
    kinds.reserve(count);
    for (std::size_t at = 0; at < count; at += 1) {
        NewMediaKind kind;
        kind.id = detail::text(remanence_new_media_id(at));
        kind.name = detail::text(remanence_new_media_name(at));
        kind.article = detail::text(remanence_new_media_article(at));
        kind.takes_geometry = remanence_new_media_takes_geometry(at);
        kinds.push_back(kind);
    }
    return kinds;
}

/// A catalog entry that is a stable spelling and a name for a user.
struct CatalogEntry {
    std::string_view id;
    std::string_view name;
};

inline std::vector<CatalogEntry> partition_schemes()
{
    std::vector<CatalogEntry> schemes;
    const std::size_t count = remanence_partition_scheme_count();
    schemes.reserve(count);
    for (std::size_t at = 0; at < count; at += 1) {
        schemes.push_back({detail::text(remanence_partition_scheme_id(at)),
                           detail::text(remanence_partition_scheme_name(at))});
    }
    return schemes;
}

inline std::vector<CatalogEntry> partition_types()
{
    std::vector<CatalogEntry> types;
    const std::size_t count = remanence_partition_type_count();
    types.reserve(count);
    for (std::size_t at = 0; at < count; at += 1) {
        types.push_back({detail::text(remanence_partition_type_id(at)),
                         detail::text(remanence_partition_type_name(at))});
    }
    return types;
}

/// The conditions a withheld operation may name as its rule (P28).
inline std::vector<std::string_view> assurance_conditions()
{
    std::vector<std::string_view> conditions;
    const std::size_t count = remanence_assurance_condition_count();
    conditions.reserve(count);
    for (std::size_t at = 0; at < count; at += 1) {
        conditions.push_back(detail::text(remanence_assurance_condition_name(at)));
    }
    return conditions;
}

/// The sources a geometry reading may be taken from.
inline std::vector<std::string_view> geometry_sources()
{
    std::vector<std::string_view> sources;
    const std::size_t count = remanence_geometry_source_count();
    sources.reserve(count);
    for (std::size_t at = 0; at < count; at += 1) {
        sources.push_back(detail::text(remanence_geometry_source_name(at)));
    }
    return sources;
}

// ---------------------------------------------------------------------
// Assurance — what one open established (P28)
// ---------------------------------------------------------------------

/// A span of the artifact that is readable, in bytes.
struct ByteRange {
    std::uint64_t start;
    std::uint64_t end;
};

/// One open's assurance state: the outcome, the condition a withheld
/// operation names as its rule, the ordered evidence, the readable
/// extents, and the effective access mode.
class Assurance : public detail::Held<RemanenceAssurance> {
public:
    using Held::Held;

    AssuranceOutcome outcome() const noexcept
    {
        return static_cast<AssuranceOutcome>(remanence_assurance_outcome(get()));
    }

    /// Whose open this medium's claim is.
    Claim claim() const noexcept
    {
        return static_cast<Claim>(remanence_assurance_claim(get()));
    }

    /// The condition behind a shortfall, absent where nothing withheld.
    std::optional<std::string> condition() const
    {
        return detail::optional_copied(remanence_assurance_condition(get()));
    }

    AccessMode access_mode() const noexcept
    {
        return static_cast<AccessMode>(remanence_assurance_access_mode(get()));
    }

    std::size_t evidence_count() const noexcept
    {
        return remanence_assurance_evidence_count(get());
    }

    std::string evidence(std::size_t index) const
    {
        detail::in_range(index, evidence_count(), "assurance evidence index");
        return detail::copied(remanence_assurance_evidence(get(), index));
    }

    /// Every evidence line, in the order the open recorded them (P4).
    std::vector<std::string> evidence() const
    {
        const std::size_t count = evidence_count();
        std::vector<std::string> lines;
        lines.reserve(count);
        for (std::size_t at = 0; at < count; at += 1) {
            lines.push_back(detail::copied(remanence_assurance_evidence(get(), at)));
        }
        return lines;
    }

    std::size_t readable_count() const noexcept
    {
        return remanence_assurance_readable_count(get());
    }

    ByteRange readable(std::size_t index) const
    {
        detail::in_range(index, readable_count(), "assurance readable index");
        ByteRange range{0, 0};
        remanence_assurance_readable(get(), index, &range.start, &range.end);
        return range;
    }

    /// The exact extents this open can read.
    std::vector<ByteRange> readable() const
    {
        const std::size_t count = readable_count();
        std::vector<ByteRange> ranges;
        ranges.reserve(count);
        for (std::size_t at = 0; at < count; at += 1) {
            ByteRange range{0, 0};
            remanence_assurance_readable(get(), at, &range.start, &range.end);
            ranges.push_back(range);
        }
        return ranges;
    }

    /// What the artifact declares its own extent to be.
    std::optional<std::uint64_t> declared_bytes() const noexcept
    {
        std::uint64_t value = 0;
        if (!remanence_assurance_declared_bytes(get(), &value)) {
            return std::nullopt;
        }
        return value;
    }

    /// What the host says is there.
    std::optional<std::uint64_t> observed_bytes() const noexcept
    {
        std::uint64_t value = 0;
        if (!remanence_assurance_observed_bytes(get(), &value)) {
            return std::nullopt;
        }
        return value;
    }

    std::optional<std::uint64_t> first_unavailable_byte() const noexcept
    {
        std::uint64_t value = 0;
        if (!remanence_assurance_first_unavailable_byte(get(), &value)) {
            return std::nullopt;
        }
        return value;
    }
};

// ---------------------------------------------------------------------
// Identification — the layers of an artifact's nesting
// ---------------------------------------------------------------------

/// One per-track entry of a variable sector layout.
struct TrackEntry {
    std::uint32_t cylinder;
    std::uint32_t side;
    std::uint32_t sectors;
    std::optional<std::uint64_t> sector_bytes;
};

/// One recognized layer, borrowed from the identification that holds it.
///
/// The layout accessors answer for the layout kind they belong to and
/// stay empty for every other, which is what `layout_kind()` says in
/// advance.
class Layer {
public:
    Layer(const RemanenceIdentification* identification, std::size_t index) noexcept
        : identification_(identification), index_(index)
    {
    }

    std::size_t index() const noexcept { return index_; }

    LayerKind kind() const noexcept
    {
        return static_cast<LayerKind>(remanence_layer_kind(identification_, index_));
    }

    /// The layer's stable spelling — `h8d`, `zip`, `hdos`.
    std::optional<std::string> id() const
    {
        return detail::optional_copied(remanence_layer_id(identification_, index_));
    }

    std::optional<std::string> name() const
    {
        return detail::optional_copied(remanence_layer_name(identification_, index_));
    }

    /// Detection confidence, 0-100.
    std::uint8_t confidence() const noexcept
    {
        return remanence_layer_confidence(identification_, index_);
    }

    /// Whether the layer matched a known format.
    bool known() const noexcept { return remanence_layer_known(identification_, index_); }

    std::optional<std::uint64_t> current_bytes() const noexcept
    {
        std::uint64_t value = 0;
        if (!remanence_layer_current_bytes(identification_, index_, &value)) {
            return std::nullopt;
        }
        return value;
    }

    std::optional<std::uint64_t> expected_bytes() const noexcept
    {
        std::uint64_t value = 0;
        if (!remanence_layer_expected_bytes(identification_, index_, &value)) {
            return std::nullopt;
        }
        return value;
    }

    /// Which of the layout groups below this layer carries.
    LayoutKind layout_kind() const noexcept
    {
        return static_cast<LayoutKind>(remanence_layer_layout_kind(identification_, index_));
    }

    // --- archive layout
    std::optional<std::string> archive_path() const
    {
        return detail::optional_copied(remanence_layer_archive_path(identification_, index_));
    }

    std::optional<std::string> archive_entry_name() const
    {
        return detail::optional_copied(remanence_layer_archive_entry_name(identification_, index_));
    }

    std::optional<std::uint64_t> archive_compressed_size() const noexcept
    {
        std::uint64_t value = 0;
        if (!remanence_layer_archive_compressed_size(identification_, index_, &value)) {
            return std::nullopt;
        }
        return value;
    }

    std::optional<std::uint64_t> archive_uncompressed_size() const noexcept
    {
        std::uint64_t value = 0;
        if (!remanence_layer_archive_uncompressed_size(identification_, index_, &value)) {
            return std::nullopt;
        }
        return value;
    }

    // --- image layout
    std::optional<std::uint64_t> image_payload_offset() const noexcept
    {
        std::uint64_t value = 0;
        if (!remanence_layer_image_payload_offset(identification_, index_, &value)) {
            return std::nullopt;
        }
        return value;
    }

    std::optional<std::uint64_t> image_payload_length() const noexcept
    {
        std::uint64_t value = 0;
        if (!remanence_layer_image_payload_length(identification_, index_, &value)) {
            return std::nullopt;
        }
        return value;
    }

    // --- physical media layout
    bool has_disk_layout() const noexcept
    {
        return remanence_layer_has_disk_layout(identification_, index_);
    }

    std::optional<std::string> disk_article() const
    {
        return detail::optional_copied(remanence_layer_disk_article(identification_, index_));
    }

    std::optional<std::uint64_t> disk_sector_size() const noexcept
    {
        std::uint64_t value = 0;
        if (!remanence_layer_disk_sector_size(identification_, index_, &value)) {
            return std::nullopt;
        }
        return value;
    }

    std::optional<std::uint32_t> disk_cylinders() const noexcept
    {
        std::uint32_t value = 0;
        if (!remanence_layer_disk_cylinders(identification_, index_, &value)) {
            return std::nullopt;
        }
        return value;
    }

    std::optional<std::uint32_t> disk_sides() const noexcept
    {
        std::uint32_t value = 0;
        if (!remanence_layer_disk_sides(identification_, index_, &value)) {
            return std::nullopt;
        }
        return value;
    }

    SectorLayoutKind disk_sector_layout_kind() const noexcept
    {
        return static_cast<SectorLayoutKind>(
            remanence_layer_disk_sector_layout_kind(identification_, index_));
    }

    /// Sectors per track for a fixed layout; 0 otherwise.
    std::uint32_t disk_sectors_per_track() const noexcept
    {
        return remanence_layer_disk_sectors_per_track(identification_, index_);
    }

    std::size_t disk_track_count() const noexcept
    {
        return remanence_layer_disk_track_count(identification_, index_);
    }

    /// One per-track entry of a variable layout.
    TrackEntry disk_track(std::size_t track_index) const
    {
        detail::in_range(track_index, disk_track_count(), "disk track index");
        TrackEntry entry{0, 0, 0, std::nullopt};
        bool has_sector_size = false;
        std::uint64_t sector_bytes = 0;
        remanence_layer_disk_track(identification_, index_, track_index, &entry.cylinder,
                                   &entry.side, &entry.sectors, &has_sector_size, &sector_bytes);
        if (has_sector_size) {
            entry.sector_bytes = sector_bytes;
        }
        return entry;
    }

    std::vector<TrackEntry> disk_tracks() const
    {
        const std::size_t count = disk_track_count();
        std::vector<TrackEntry> tracks;
        tracks.reserve(count);
        for (std::size_t at = 0; at < count; at += 1) {
            tracks.push_back(disk_track(at));
        }
        return tracks;
    }

    std::optional<std::uint64_t> disk_total_sectors() const noexcept
    {
        std::uint64_t value = 0;
        if (!remanence_layer_disk_total_sectors(identification_, index_, &value)) {
            return std::nullopt;
        }
        return value;
    }

    // --- filesystem layout
    std::optional<std::uint64_t> fs_offset_bytes() const noexcept
    {
        std::uint64_t value = 0;
        if (!remanence_layer_fs_offset_bytes(identification_, index_, &value)) {
            return std::nullopt;
        }
        return value;
    }

    std::optional<std::uint64_t> fs_length_bytes() const noexcept
    {
        std::uint64_t value = 0;
        if (!remanence_layer_fs_length_bytes(identification_, index_, &value)) {
            return std::nullopt;
        }
        return value;
    }

private:
    const RemanenceIdentification* identification_;
    std::size_t index_;
};

/// What identifying an artifact recognized: its nesting layers, and the
/// evidence every verdict rests on (P4).
class Identification : public detail::Held<RemanenceIdentification> {
public:
    using Held::Held;

    /// Whether the medium reported unsaved modifications at identify time.
    bool modified() const noexcept { return remanence_identification_modified(get()); }

    std::size_t layer_count() const noexcept
    {
        return remanence_identification_layer_count(get());
    }

    // **Deleted on a temporary.** A `Layer` borrows the identification
    // it came from, so `medium.identify().layers()` would hand back
    // views of a handle that died at the end of the expression. The
    // `const&&` overload makes that a compile error rather than a
    // dangling read; bind the handle to a name and it compiles. Every
    // accessor below that answers a borrowed record does the same.
    Layer layer(std::size_t index) const&& = delete;
    Layer layer(std::size_t index) const&
    {
        detail::in_range(index, layer_count(), "layer index");
        return Layer(get(), index);
    }

    std::vector<Layer> layers() const&& = delete;
    std::vector<Layer> layers() const&
    {
        const std::size_t count = layer_count();
        std::vector<Layer> found;
        found.reserve(count);
        for (std::size_t at = 0; at < count; at += 1) {
            found.push_back(Layer(get(), at));
        }
        return found;
    }

    std::size_t evidence_count() const noexcept
    {
        return remanence_identification_evidence_count(get());
    }

    std::string evidence(std::size_t index) const
    {
        detail::in_range(index, evidence_count(), "identification evidence index");
        return detail::copied(remanence_identification_evidence(get(), index));
    }

    std::vector<std::string> evidence() const
    {
        const std::size_t count = evidence_count();
        std::vector<std::string> lines;
        lines.reserve(count);
        for (std::size_t at = 0; at < count; at += 1) {
            lines.push_back(detail::copied(remanence_identification_evidence(get(), at)));
        }
        return lines;
    }
};

// ---------------------------------------------------------------------
// Geometry — the recording's own coordinates, as evidence
// ---------------------------------------------------------------------

/// A whole set of recording coordinates.
struct Coordinates {
    std::uint32_t cylinders;
    std::uint32_t heads;
    std::uint32_t sectors_per_track;
    std::uint64_t sector_bytes;
};

/// One reading of a medium's geometry, kept with where it was taken.
///
/// A reading states the parts its source stated and no others, which is
/// why every part is optional here.
class GeometryReading {
public:
    GeometryReading(const RemanenceGeometry* geometry, std::size_t index) noexcept
        : geometry_(geometry), index_(index)
    {
    }

    std::size_t index() const noexcept { return index_; }

    /// Which enumerated source this reading came from.
    ///
    /// The three accessors here qualify the helper namespace in full,
    /// because `detail()` below is a member of this class and would
    /// otherwise hide it. Keeping the ABI's own spelling is worth the
    /// two qualifications.
    std::string source() const
    {
        return ::remanence::detail::copied(remanence_geometry_reading_source(geometry_, index_));
    }

    /// Where in the artifact it was taken.
    std::optional<std::string> at() const
    {
        return ::remanence::detail::optional_copied(
            remanence_geometry_reading_at(geometry_, index_));
    }

    /// What that source said, in its own terms.
    std::optional<std::string> detail() const
    {
        return ::remanence::detail::optional_copied(
            remanence_geometry_reading_detail(geometry_, index_));
    }

    std::optional<std::uint32_t> cylinders() const noexcept
    {
        std::uint32_t value = 0;
        if (!remanence_geometry_reading_cylinders(geometry_, index_, &value)) {
            return std::nullopt;
        }
        return value;
    }

    std::optional<std::uint32_t> heads() const noexcept
    {
        std::uint32_t value = 0;
        if (!remanence_geometry_reading_heads(geometry_, index_, &value)) {
            return std::nullopt;
        }
        return value;
    }

    std::optional<std::uint32_t> sectors_per_track() const noexcept
    {
        std::uint32_t value = 0;
        if (!remanence_geometry_reading_sectors_per_track(geometry_, index_, &value)) {
            return std::nullopt;
        }
        return value;
    }

    std::optional<std::uint64_t> sector_bytes() const noexcept
    {
        std::uint64_t value = 0;
        if (!remanence_geometry_reading_sector_bytes(geometry_, index_, &value)) {
            return std::nullopt;
        }
        return value;
    }

private:
    const RemanenceGeometry* geometry_;
    std::size_t index_;
};

/// One medium's geometry as the evidence left it: what the readings
/// settle between them, what they disagree on, and what nothing stated.
class Geometry : public detail::Held<RemanenceGeometry> {
public:
    using Held::Held;

    GeometryState state() const noexcept
    {
        return static_cast<GeometryState>(remanence_geometry_state(get()));
    }

    /// The settled coordinates, present only where every part is
    /// established and the readings agree.
    std::optional<Coordinates> coordinates() const noexcept
    {
        Coordinates found{0, 0, 0, 0};
        if (!remanence_geometry_coordinates(get(), &found.cylinders, &found.heads,
                                            &found.sectors_per_track, &found.sector_bytes)) {
            return std::nullopt;
        }
        return found;
    }

    /// The parts two sources state different values for.
    std::vector<std::string> conflicts() const
    {
        const std::size_t count = remanence_geometry_conflict_count(get());
        std::vector<std::string> parts;
        parts.reserve(count);
        for (std::size_t at = 0; at < count; at += 1) {
            parts.push_back(detail::copied(remanence_geometry_conflict(get(), at)));
        }
        return parts;
    }

    /// The parts nothing beneath the medium stated at all.
    std::vector<std::string> unsettled() const
    {
        const std::size_t count = remanence_geometry_unsettled_count(get());
        std::vector<std::string> parts;
        parts.reserve(count);
        for (std::size_t at = 0; at < count; at += 1) {
            parts.push_back(detail::copied(remanence_geometry_unsettled(get(), at)));
        }
        return parts;
    }

    std::size_t reading_count() const noexcept { return remanence_geometry_reading_count(get()); }

    // Deleted on a temporary, for the reason `Identification::layer`
    // states: these records borrow the handle they came from.
    GeometryReading reading(std::size_t index) const&& = delete;
    GeometryReading reading(std::size_t index) const&
    {
        detail::in_range(index, reading_count(), "geometry reading index");
        return GeometryReading(get(), index);
    }

    std::vector<GeometryReading> readings() const&& = delete;
    std::vector<GeometryReading> readings() const&
    {
        const std::size_t count = reading_count();
        std::vector<GeometryReading> found;
        found.reserve(count);
        for (std::size_t at = 0; at < count; at += 1) {
            found.push_back(GeometryReading(get(), at));
        }
        return found;
    }
};

// ---------------------------------------------------------------------
// Directory listings
// ---------------------------------------------------------------------

/// One fact the recognizing filesystem declares about an entry, in that
/// filesystem's own spelling. Nothing is normalized on the way through.
struct DeclaredFact {
    std::string key;
    std::string value;
};

/// One entry of a listing, borrowed from the listing that holds it.
class Entry {
public:
    Entry(const RemanenceEntryList* list, std::size_t index) noexcept : list_(list), index_(index)
    {
    }

    std::size_t index() const noexcept { return index_; }

    /// The name as the filesystem stores it.
    std::string name() const
    {
        return detail::copied(remanence_entry_name(list_, index_));
    }

    EntryKind kind() const noexcept
    {
        return static_cast<EntryKind>(remanence_entry_kind(list_, index_));
    }

    std::uint64_t size_bytes() const noexcept { return remanence_entry_size_bytes(list_, index_); }

    std::size_t declared_count() const noexcept
    {
        return remanence_entry_declared_count(list_, index_);
    }

    DeclaredFact declared(std::size_t fact) const
    {
        detail::in_range(fact, declared_count(), "declared fact index");
        return {detail::copied(remanence_entry_declared_key(list_, index_, fact)),
                detail::copied(remanence_entry_declared_value(list_, index_, fact))};
    }

    /// Everything the filesystem declares beyond name, kind and size.
    std::vector<DeclaredFact> declared() const
    {
        const std::size_t count = declared_count();
        std::vector<DeclaredFact> facts;
        facts.reserve(count);
        for (std::size_t at = 0; at < count; at += 1) {
            facts.push_back({detail::copied(remanence_entry_declared_key(list_, index_, at)),
                             detail::copied(remanence_entry_declared_value(list_, index_, at))});
        }
        return facts;
    }

private:
    const RemanenceEntryList* list_;
    std::size_t index_;
};

/// A directory listing.
class EntryList : public detail::Held<RemanenceEntryList> {
public:
    using Held::Held;

    std::size_t size() const noexcept { return remanence_entry_count(get()); }
    bool empty() const noexcept { return size() == 0; }

    // Deleted on a temporary, for the reason `Identification::layer`
    // states: these records borrow the handle they came from.
    Entry at(std::size_t index) const&& = delete;
    Entry at(std::size_t index) const&
    {
        detail::in_range(index, size(), "entry index");
        return Entry(get(), index);
    }

    Entry operator[](std::size_t index) const&& = delete;
    Entry operator[](std::size_t index) const& { return Entry(get(), index); }

    std::vector<Entry> entries() const&& = delete;
    std::vector<Entry> entries() const&
    {
        const std::size_t count = size();
        std::vector<Entry> found;
        found.reserve(count);
        for (std::size_t at = 0; at < count; at += 1) {
            found.push_back(Entry(get(), at));
        }
        return found;
    }
};

// ---------------------------------------------------------------------
// File bytes and load sources
// ---------------------------------------------------------------------

/// Bytes read out of a volume or catalog, owned by this handle.
class FileData : public detail::Held<RemanenceFileData> {
public:
    using Held::Held;

    const std::uint8_t* data() const noexcept
    {
        std::size_t length = 0;
        return remanence_file_data_bytes(get(), &length);
    }

    std::size_t size() const noexcept
    {
        std::size_t length = 0;
        remanence_file_data_bytes(get(), &length);
        return length;
    }

    bool empty() const noexcept { return size() == 0; }

    /// A copy that outlives this handle.
    std::vector<std::uint8_t> to_vector() const
    {
        std::size_t length = 0;
        const std::uint8_t* bytes = remanence_file_data_bytes(get(), &length);
        if (bytes == nullptr) {
            return {};
        }
        return std::vector<std::uint8_t>(bytes, bytes + length);
    }
};

/// One file taken from an archive medium's namespace as a load's source,
/// riding the claim of the medium it came from.
///
/// A load consumes it: `Session::load_media_source` takes it by value
/// and the handle is spent, exactly as the ABI's is.
class FileSource : public detail::Held<RemanenceFileSource> {
public:
    using Held::Held;

    /// The name the namespace holds this source's file under.
    std::string name() const
    {
        return detail::copied(remanence_file_source_name(get()));
    }

    std::uint64_t size_bytes() const noexcept { return remanence_file_source_size_bytes(get()); }
};

/// Every file gathered under one namespace path as a load's sources —
/// what a collection format reads. Consumed by
/// `Session::load_media_sources`.
class FileSourceList : public detail::Held<RemanenceFileSourceList> {
public:
    using Held::Held;

    std::size_t size() const noexcept { return remanence_file_source_list_count(get()); }
    bool empty() const noexcept { return size() == 0; }

    std::string name(std::size_t index) const
    {
        detail::in_range(index, size(), "file source index");
        return detail::copied(remanence_file_source_list_name(get(), index));
    }

    std::vector<std::string> names() const
    {
        const std::size_t count = size();
        std::vector<std::string> found;
        found.reserve(count);
        for (std::size_t at = 0; at < count; at += 1) {
            found.push_back(detail::copied(remanence_file_source_list_name(get(), at)));
        }
        return found;
    }
};

/// One file, named by the filesystem that holds it.
class File : public detail::Held<RemanenceFile> {
public:
    using Held::Held;

    /// The path this file was reached by.
    std::string path() const { return detail::copied(remanence_file_path(get())); }

    /// The name as the filesystem stores it, which is not always the
    /// spelling the caller asked by.
    std::string name() const { return detail::copied(remanence_file_name(get())); }

    std::uint64_t size_bytes() const noexcept { return remanence_file_size_bytes(get()); }

    EntryKind kind() const noexcept
    {
        return static_cast<EntryKind>(remanence_file_kind(get()));
    }

    /// The whole file, copied out.
    FileData bytes() const
    {
        detail::Outcome outcome;
        RemanenceFileData* data =
            remanence_file_bytes(get(), outcome.category(), outcome.message(), outcome.rule());
        return FileData(outcome.require(data, "the file could not be read"));
    }

    /// Exactly `length` bytes at `offset`, which must lie within the file.
    void read_at(std::uint64_t offset, std::uint8_t* buffer, std::size_t length) const
    {
        detail::Outcome outcome;
        outcome.require(remanence_file_read_at(get(), offset, buffer, length, outcome.category(),
                                               outcome.message(), outcome.rule()),
                        "the file could not be read at that offset");
    }

    std::vector<std::uint8_t> read_at(std::uint64_t offset, std::size_t length) const
    {
        std::vector<std::uint8_t> buffer(length);
        if (length > 0) {
            read_at(offset, buffer.data(), length);
        }
        return buffer;
    }

    /// Writes in place, within the file's current size. Buffered until
    /// the medium commits.
    void write_at(std::uint64_t offset, const std::uint8_t* bytes, std::size_t length) const
    {
        detail::Outcome outcome;
        outcome.require(remanence_file_write_at(get(), offset, bytes, length, outcome.category(),
                                                outcome.message(), outcome.rule()),
                        "the file could not be written at that offset");
    }

    void write_at(std::uint64_t offset, const std::vector<std::uint8_t>& bytes) const
    {
        write_at(offset, bytes.data(), bytes.size());
    }

    /// This file taken as a load's source. Minted from an archive's
    /// namespace alone; a volume-backed file is refused by name.
    FileSource source()
    {
        detail::Outcome outcome;
        RemanenceFileSource* made =
            remanence_file_source(get(), outcome.category(), outcome.message(), outcome.rule());
        return FileSource(outcome.require(made, "the file is no load's source"));
    }
};

// ---------------------------------------------------------------------
// The inspection report — one disk's layered snapshot
//
// A snapshot rather than a borrow: the report owns everything reached
// through it, and the records below are views into it that die with it.
// ---------------------------------------------------------------------

/// One region a schema declared, as the report holds it.
class ReportRegion {
public:
    ReportRegion(const RemanenceDiskReport* report, std::size_t index) noexcept
        : report_(report), index_(index)
    {
    }

    std::size_t index() const noexcept { return index_; }

    /// The opaque layout-derived identity a volume's origin names.
    std::uint64_t id() const noexcept { return remanence_report_region_id(report_, index_); }

    /// The number the scheme itself declared for it.
    std::uint32_t declared_number() const noexcept
    {
        return remanence_report_region_declared_number(report_, index_);
    }

    std::optional<std::string> declared_placement() const
    {
        return detail::optional_copied(remanence_report_region_declared_placement(report_, index_));
    }

    RegionRole role() const noexcept
    {
        return static_cast<RegionRole>(remanence_report_region_role(report_, index_));
    }

    std::uint8_t declared_type() const noexcept
    {
        return remanence_report_region_declared_type(report_, index_);
    }

    std::optional<std::string> declared_type_reading() const
    {
        return detail::optional_copied(
            remanence_report_region_declared_type_reading(report_, index_));
    }

    /// Whether composition claimed this region.
    bool is_claimed() const noexcept
    {
        return remanence_report_region_is_claimed(report_, index_);
    }

    std::uint64_t start_bytes() const noexcept
    {
        return remanence_report_region_start_bytes(report_, index_);
    }

    std::uint64_t length_bytes() const noexcept
    {
        return remanence_report_region_length_bytes(report_, index_);
    }

    std::optional<ErrorCategory> issue_category() const noexcept
    {
        RemanenceErrorCategory category = REMANENCE_ERROR_CATEGORY_IO;
        if (!remanence_report_region_issue_category(report_, index_, &category)) {
            return std::nullopt;
        }
        return static_cast<ErrorCategory>(category);
    }

    std::optional<std::string> issue() const
    {
        return detail::optional_copied(remanence_report_region_issue(report_, index_));
    }

private:
    const RemanenceDiskReport* report_;
    std::size_t index_;
};

/// One composed volume, as the report holds it.
class ReportVolume {
public:
    ReportVolume(const RemanenceDiskReport* report, std::size_t index) noexcept
        : report_(report), index_(index)
    {
    }

    std::size_t index() const noexcept { return index_; }

    std::uint64_t id() const noexcept { return remanence_report_volume_id(report_, index_); }

    VolumeOrigin origin() const noexcept
    {
        return static_cast<VolumeOrigin>(remanence_report_volume_origin(report_, index_));
    }

    /// The regions this volume's storage came from, by their identities.
    std::vector<std::uint64_t> origin_regions() const
    {
        const std::size_t count = remanence_report_volume_origin_region_count(report_, index_);
        std::vector<std::uint64_t> regions;
        regions.reserve(count);
        for (std::size_t at = 0; at < count; at += 1) {
            regions.push_back(remanence_report_volume_origin_region_id(report_, index_, at));
        }
        return regions;
    }

    std::uint64_t start_bytes() const noexcept
    {
        return remanence_report_volume_start_bytes(report_, index_);
    }

    std::uint64_t length_bytes() const noexcept
    {
        return remanence_report_volume_length_bytes(report_, index_);
    }

    std::vector<std::string> evidence() const
    {
        const std::size_t count = remanence_report_volume_evidence_count(report_, index_);
        std::vector<std::string> lines;
        lines.reserve(count);
        for (std::size_t at = 0; at < count; at += 1) {
            lines.push_back(detail::copied(remanence_report_volume_evidence(report_, index_, at)));
        }
        return lines;
    }

private:
    const RemanenceDiskReport* report_;
    std::size_t index_;
};

/// One source the recognizing filesystem consulted for a label, and what
/// it held. `present` is the third state — no such field at all — and is
/// distinct from a field that is present and blank.
struct LabelReading {
    std::string source;
    bool present;
    std::optional<std::string> stored;
};

/// One issue a recognition carries.
struct RecognitionIssue {
    std::optional<ErrorCategory> category;
    std::optional<std::string> diagnostic;
};

/// One recognized filesystem, as the report holds it.
class ReportFilesystem {
public:
    ReportFilesystem(const RemanenceDiskReport* report, std::size_t index) noexcept
        : report_(report), index_(index)
    {
    }

    std::size_t index() const noexcept { return index_; }

    std::uint64_t id() const noexcept { return remanence_report_filesystem_id(report_, index_); }

    /// The volume this filesystem was recognized on.
    std::uint64_t volume_id() const noexcept
    {
        return remanence_report_filesystem_volume_id(report_, index_);
    }

    std::optional<std::string> kind() const
    {
        return detail::optional_copied(remanence_report_filesystem_kind(report_, index_));
    }

    /// Whether the format gives this filesystem a label at all.
    bool label_answered() const noexcept
    {
        return remanence_report_filesystem_label_answered(report_, index_);
    }

    std::optional<std::string> label() const
    {
        return detail::optional_copied(remanence_report_filesystem_label(report_, index_));
    }

    /// Which source the answer was taken from.
    std::optional<std::string> label_answered_by() const
    {
        return detail::optional_copied(
            remanence_report_filesystem_label_answered_by(report_, index_));
    }

    /// Every source consulted, in the order the format's policy consults
    /// them (P4).
    std::vector<LabelReading> label_readings() const
    {
        const std::size_t count =
            remanence_report_filesystem_label_reading_count(report_, index_);
        std::vector<LabelReading> readings;
        readings.reserve(count);
        for (std::size_t at = 0; at < count; at += 1) {
            readings.push_back(
                {detail::copied(
                     remanence_report_filesystem_label_reading_source(report_, index_, at)),
                 remanence_report_filesystem_label_reading_present(report_, index_, at),
                 detail::optional_copied(
                     remanence_report_filesystem_label_reading_stored(report_, index_, at))});
        }
        return readings;
    }

    std::optional<std::uint64_t> cluster_bytes() const noexcept
    {
        std::uint64_t value = 0;
        if (!remanence_report_filesystem_cluster_bytes(report_, index_, &value)) {
            return std::nullopt;
        }
        return value;
    }

    std::optional<std::uint64_t> cluster_count() const noexcept
    {
        std::uint64_t value = 0;
        if (!remanence_report_filesystem_cluster_count(report_, index_, &value)) {
            return std::nullopt;
        }
        return value;
    }

    /// Sectors per track as the filesystem's own structures declare it —
    /// a filesystem declaration, which manufactures no physical drive.
    std::optional<std::uint16_t> sectors_per_track() const noexcept
    {
        std::uint16_t value = 0;
        if (!remanence_report_filesystem_sectors_per_track(report_, index_, &value)) {
            return std::nullopt;
        }
        return value;
    }

    std::optional<std::uint16_t> heads() const noexcept
    {
        std::uint16_t value = 0;
        if (!remanence_report_filesystem_heads(report_, index_, &value)) {
            return std::nullopt;
        }
        return value;
    }

    /// Cylinders, only where the derivation is exact. Never invented.
    std::optional<std::uint64_t> cylinders() const noexcept
    {
        std::uint64_t value = 0;
        if (!remanence_report_filesystem_cylinders(report_, index_, &value)) {
            return std::nullopt;
        }
        return value;
    }

    std::vector<RecognitionIssue> issues() const
    {
        const std::size_t count = remanence_report_filesystem_issue_count(report_, index_);
        std::vector<RecognitionIssue> found;
        found.reserve(count);
        for (std::size_t at = 0; at < count; at += 1) {
            RemanenceErrorCategory category = REMANENCE_ERROR_CATEGORY_IO;
            std::optional<ErrorCategory> reported;
            if (remanence_report_filesystem_issue_category(report_, index_, at, &category)) {
                reported = static_cast<ErrorCategory>(category);
            }
            found.push_back({reported, detail::optional_copied(remanence_report_filesystem_issue(
                                           report_, index_, at))});
        }
        return found;
    }

private:
    const RemanenceDiskReport* report_;
    std::size_t index_;
};

/// A snapshot of one disk's layered inspection: the device, what its
/// leading structure turned out to be, the schema, the regions, the
/// volumes composed over them, and the filesystems recognized on those.
class DiskReport : public detail::Held<RemanenceDiskReport> {
public:
    using Held::Held;

    std::uint64_t device_id() const noexcept { return remanence_report_device_id(get()); }

    std::optional<std::string> device_image_format() const
    {
        return detail::optional_copied(remanence_report_device_image_format(get()));
    }

    std::uint64_t device_length_bytes() const noexcept
    {
        return remanence_report_device_length_bytes(get());
    }

    std::optional<std::string> device_article() const
    {
        return detail::optional_copied(remanence_report_device_article(get()));
    }

    std::optional<std::string> device_type() const
    {
        return detail::optional_copied(remanence_report_device_type(get()));
    }

    /// The P13 authoritative layer, and the P23 active one.
    std::optional<std::string> device_authoritative_layer() const
    {
        return detail::optional_copied(remanence_report_device_authoritative_layer(get()));
    }

    std::optional<std::string> device_active_layer() const
    {
        return detail::optional_copied(remanence_report_device_active_layer(get()));
    }

    DiskContent content() const noexcept
    {
        return static_cast<DiskContent>(remanence_report_content(get()));
    }

    std::optional<std::string> content_evidence() const
    {
        return detail::optional_copied(remanence_report_content_evidence(get()));
    }

    bool has_partition_schema() const noexcept
    {
        return remanence_report_has_partition_schema(get());
    }

    std::optional<std::string> partition_schema_kind() const
    {
        return detail::optional_copied(remanence_report_partition_schema_kind(get()));
    }

    std::vector<std::string> partition_schema_evidence() const
    {
        const std::size_t count = remanence_report_partition_schema_evidence_count(get());
        std::vector<std::string> lines;
        lines.reserve(count);
        for (std::size_t at = 0; at < count; at += 1) {
            lines.push_back(detail::copied(remanence_report_partition_schema_evidence(get(), at)));
        }
        return lines;
    }

    std::size_t region_count() const noexcept { return remanence_report_region_count(get()); }

    // Deleted on a temporary, for the reason `Identification::layer`
    // states: these records borrow the handle they came from.
    ReportRegion region(std::size_t index) const&& = delete;
    ReportRegion region(std::size_t index) const&
    {
        detail::in_range(index, region_count(), "report region index");
        return ReportRegion(get(), index);
    }

    std::vector<ReportRegion> regions() const&& = delete;
    std::vector<ReportRegion> regions() const&
    {
        const std::size_t count = region_count();
        std::vector<ReportRegion> found;
        found.reserve(count);
        for (std::size_t at = 0; at < count; at += 1) {
            found.push_back(ReportRegion(get(), at));
        }
        return found;
    }

    std::size_t volume_count() const noexcept { return remanence_report_volume_count(get()); }

    /// How many volumes bear a filesystem this release can read.
    std::size_t readable_filesystem_volume_count() const noexcept
    {
        return remanence_report_readable_filesystem_volume_count(get());
    }

    ReportVolume volume(std::size_t index) const&& = delete;
    ReportVolume volume(std::size_t index) const&
    {
        detail::in_range(index, volume_count(), "report volume index");
        return ReportVolume(get(), index);
    }

    std::vector<ReportVolume> volumes() const&& = delete;
    std::vector<ReportVolume> volumes() const&
    {
        const std::size_t count = volume_count();
        std::vector<ReportVolume> found;
        found.reserve(count);
        for (std::size_t at = 0; at < count; at += 1) {
            found.push_back(ReportVolume(get(), at));
        }
        return found;
    }

    std::size_t filesystem_count() const noexcept
    {
        return remanence_report_filesystem_count(get());
    }

    ReportFilesystem filesystem(std::size_t index) const&& = delete;
    ReportFilesystem filesystem(std::size_t index) const&
    {
        detail::in_range(index, filesystem_count(), "report filesystem index");
        return ReportFilesystem(get(), index);
    }

    std::vector<ReportFilesystem> filesystems() const&& = delete;
    std::vector<ReportFilesystem> filesystems() const&
    {
        const std::size_t count = filesystem_count();
        std::vector<ReportFilesystem> found;
        found.reserve(count);
        for (std::size_t at = 0; at < count; at += 1) {
            found.push_back(ReportFilesystem(get(), at));
        }
        return found;
    }
};

// ---------------------------------------------------------------------
// Discovery — what one artifact turned out to be
// ---------------------------------------------------------------------

/// What one artifact turned out to be, and the claim under which that
/// was established.
///
/// It holds the claim until it is consumed or destroyed, and nothing is
/// created by it: no medium, no cache, no spilled backing. A load
/// consumes it — `Session::load_discovery` takes it by value — so one
/// discovery becomes one medium and no window opens between the question
/// and the load.
class Discovery : public detail::Held<RemanenceDiscovery> {
public:
    using Held::Held;

    /// The artifact claimed — the archive itself for an image inside one.
    std::optional<std::string> path() const
    {
        return detail::optional_copied(remanence_discovery_path(get()));
    }

    /// The resolved image — the entry name for an image inside an archive.
    std::optional<std::string> image_path() const
    {
        return detail::optional_copied(remanence_discovery_image_path(get()));
    }

    /// The image format's stable spelling — `h8d`, `qcow2`, `vdi`, `raw`.
    std::optional<std::string> image_format() const
    {
        return detail::optional_copied(remanence_discovery_image_format(get()));
    }

    std::optional<std::string> image_format_name() const
    {
        return detail::optional_copied(remanence_discovery_image_format_name(get()));
    }

    /// The image container format; absent for a medium that is no disk
    /// image, an archive being the honest case.
    std::optional<DiskFormat> format() const noexcept
    {
        RemanenceDiskFormat found = REMANENCE_DISK_FORMAT_RAW;
        if (!remanence_discovery_format(get(), &found)) {
            return std::nullopt;
        }
        return static_cast<DiskFormat>(found);
    }

    /// The exact article, by the catalog's stable spelling (P14).
    std::optional<std::string> article() const
    {
        return detail::optional_copied(remanence_discovery_article(get()));
    }

    std::optional<std::string> article_name() const
    {
        return detail::optional_copied(remanence_discovery_article_name(get()));
    }

    /// Where this could go: the devices served this article. Empty means
    /// no device this release claims takes it, which is an archive's
    /// honest answer.
    std::vector<std::string> accepting_devices() const
    {
        const std::size_t count = remanence_discovery_accepting_device_count(get());
        std::vector<std::string> devices;
        devices.reserve(count);
        for (std::size_t at = 0; at < count; at += 1) {
            devices.push_back(detail::copied(remanence_discovery_accepting_device(get(), at)));
        }
        return devices;
    }

    /// The device types the recognizing format records — the set a
    /// declaration may name.
    std::vector<std::string> recorded_devices() const
    {
        const std::size_t count = remanence_discovery_recorded_device_count(get());
        std::vector<std::string> devices;
        devices.reserve(count);
        for (std::size_t at = 0; at < count; at += 1) {
            devices.push_back(detail::copied(remanence_discovery_recorded_device(get(), at)));
        }
        return devices;
    }

    /// What wrote it, or nothing where the format records several types
    /// and the artifact does not say which — in which case a load
    /// declares the type instead.
    std::optional<std::string> device_type() const
    {
        return detail::optional_copied(remanence_discovery_device_type(get()));
    }

    /// The resolved image's own size — the raw plane.
    std::uint64_t image_size_bytes() const noexcept
    {
        return remanence_discovery_image_size_bytes(get());
    }

    /// The presented disk's size, or zero for a medium that presents no
    /// disk.
    std::uint64_t size() const noexcept { return remanence_discovery_size(get()); }

    /// The effective access mode a load consuming this inherits (P28).
    AccessMode mode() const noexcept
    {
        return static_cast<AccessMode>(remanence_discovery_mode(get()));
    }

    /// What this established about the evidence beneath the medium,
    /// before anything is read.
    Assurance assurance() const { return Assurance(remanence_discovery_assurance(get())); }

    /// The same reading a loaded medium gives.
    Identification identify() const
    {
        return Identification(remanence_discovery_identify(get()));
    }
};

/// Identifies the artifact at `path` under the caller's declared intent,
/// and answers with what it is and where it could go.
///
/// On no handle at all: no session and no machine, because it consults
/// catalogs and evidence rather than configuration, and it mutates
/// nothing (P2). A `Write` intent claims the artifact exclusively and
/// throws where it cannot, never falling back (P7).
inline Discovery discover_media(const std::string& path, AccessIntent intent = AccessIntent::Read)
{
    detail::Outcome outcome;
    RemanenceDiscovery* found =
        remanence_discover_media(path.c_str(), static_cast<RemanenceAccessIntent>(intent),
                                 outcome.category(), outcome.message(), outcome.rule());
    return Discovery(outcome.require(found, "the artifact could not be discovered"));
}

// ---------------------------------------------------------------------
// Spaces — the two vantages on one composed node
//
// The volume is the addressable vantage and the filesystem the namespace
// one; each is minted by its own door on a partition, and each owns the
// space handle behind it.
// ---------------------------------------------------------------------

/// One source the recognizing filesystem consulted for a label.
struct FilesystemLabelReading {
    std::string source;
    std::optional<std::string> stored;
};

/// A volume: one span of a medium's content, addressed by byte offset.
class Volume : public detail::Held<RemanenceSpace> {
public:
    using Held::Held;

    /// Whether this space answers the addressable verbs at all.
    bool is_addressable() const noexcept { return remanence_volume_is_addressable(get()); }

    /// The opaque layout-derived identity the report issued for it.
    std::uint64_t id() const noexcept { return remanence_volume_id(get()); }

    std::uint64_t start_bytes() const noexcept { return remanence_volume_start_bytes(get()); }

    std::uint64_t length_bytes() const noexcept { return remanence_volume_length_bytes(get()); }

    void read_at(std::uint64_t offset, std::uint8_t* buffer, std::size_t length)
    {
        detail::Outcome outcome;
        outcome.require(remanence_volume_read_at(get(), offset, buffer, length, outcome.category(),
                                                 outcome.message(), outcome.rule()),
                        "the volume could not be read");
    }

    std::vector<std::uint8_t> read_at(std::uint64_t offset, std::size_t length)
    {
        std::vector<std::uint8_t> buffer(length);
        if (length > 0) {
            read_at(offset, buffer.data(), length);
        }
        return buffer;
    }

    /// Buffered until the medium commits (P2).
    void write_at(std::uint64_t offset, const std::uint8_t* bytes, std::size_t length)
    {
        detail::Outcome outcome;
        outcome.require(remanence_volume_write_at(get(), offset, bytes, length, outcome.category(),
                                                  outcome.message(), outcome.rule()),
                        "the volume could not be written");
    }

    void write_at(std::uint64_t offset, const std::vector<std::uint8_t>& bytes)
    {
        write_at(offset, bytes.data(), bytes.size());
    }

    /// Every file under `path` gathered as a load's sources, from an
    /// archive's namespace alone.
    FileSourceList files(const std::string& path = std::string{})
    {
        detail::Outcome outcome;
        RemanenceFileSourceList* gathered = remanence_space_files(
            get(), path.c_str(), outcome.category(), outcome.message(), outcome.rule());
        return FileSourceList(outcome.require(gathered, "the sources could not be gathered"));
    }
};

/// A filesystem: the namespace vantage on the same composed node.
class Filesystem : public detail::Held<RemanenceSpace> {
public:
    using Held::Held;

    bool has_namespace() const noexcept { return remanence_filesystem_has_namespace(get()); }

    /// The filesystem kind in its stable spelling — `FAT12`, `hdos`.
    std::optional<std::string> kind() const
    {
        return detail::optional_copied(remanence_filesystem_kind(get()));
    }

    /// The label the recognizing filesystem read, absent where the
    /// namespace has no such field. A failure to read throws; an honest
    /// absence does not.
    std::optional<std::string> label() const
    {
        detail::Outcome outcome;
        char* read =
            remanence_filesystem_label(get(), outcome.category(), outcome.message(), outcome.rule());
        if (read == nullptr && outcome.refused()) {
            outcome.raise("the label could not be read");
        }
        return detail::owned_text(read);
    }

    /// The sources consulted for that label, in policy order (P4).
    std::vector<FilesystemLabelReading> label_readings() const
    {
        const std::size_t count = remanence_filesystem_label_reading_count(get());
        std::vector<FilesystemLabelReading> readings;
        readings.reserve(count);
        for (std::size_t at = 0; at < count; at += 1) {
            char* source = nullptr;
            char* stored = nullptr;
            if (!remanence_filesystem_label_reading(get(), at, &source, &stored)) {
                break;
            }
            readings.push_back({detail::owned_text(source).value_or(std::string{}),
                                detail::owned_text(stored)});
        }
        return readings;
    }

    /// The observations that recognized this namespace (P4).
    std::vector<std::string> evidence() const
    {
        const std::size_t count = remanence_filesystem_evidence_count(get());
        std::vector<std::string> lines;
        lines.reserve(count);
        for (std::size_t at = 0; at < count; at += 1) {
            std::optional<std::string> line =
                detail::owned_text(remanence_filesystem_evidence(get(), at));
            if (!line.has_value()) {
                break;
            }
            lines.push_back(std::move(*line));
        }
        return lines;
    }

    /// Lists a directory: `""` is the root, `A/B` descends.
    EntryList entries(const std::string& path = std::string{}) const
    {
        detail::Outcome outcome;
        RemanenceEntryList* listed = remanence_filesystem_entries(
            get(), path.c_str(), outcome.category(), outcome.message(), outcome.rule());
        return EntryList(outcome.require(listed, "the directory could not be listed"));
    }

    /// Answers one path (U3): a one-entry listing where something is
    /// there and an empty one where nothing is. Absence is an answer;
    /// only failure throws.
    EntryList stat(const std::string& path) const
    {
        detail::Outcome outcome;
        RemanenceEntryList* listed = remanence_filesystem_stat(
            get(), path.c_str(), outcome.category(), outcome.message(), outcome.rule());
        return EntryList(outcome.require(listed, "the path could not be stated"));
    }

    /// The file at `path`. This is where absence stops being an answer:
    /// nothing there, and a directory, are both refused by name.
    File get_file(const std::string& path) const
    {
        detail::Outcome outcome;
        RemanenceFile* found = remanence_filesystem_get_file(
            get(), path.c_str(), outcome.category(), outcome.message(), outcome.rule());
        return File(outcome.require(found, "there is no such file"));
    }

    /// Opens an entry as an artifact of its own — recursion being the
    /// same journey again, under the claim already held.
    Discovery discover(const std::string& path) const
    {
        detail::Outcome outcome;
        RemanenceDiscovery* found = remanence_filesystem_discover(
            get(), path.c_str(), outcome.category(), outcome.message(), outcome.rule());
        return Discovery(outcome.require(found, "that entry is no artifact of its own"));
    }

    /// A file's bytes, the whole-value convenience beside `File::read_at`.
    FileData read_file(const std::string& path) const
    {
        detail::Outcome outcome;
        RemanenceFileData* data = remanence_filesystem_read_file(
            get(), path.c_str(), outcome.category(), outcome.message(), outcome.rule());
        return FileData(outcome.require(data, "the file could not be read"));
    }

    /// Sets a file's size, creating it when absent. Buffered until commit.
    void resize_file(const std::string& path, std::uint64_t size) const
    {
        detail::Outcome outcome;
        outcome.require(remanence_filesystem_resize_file(get(), path.c_str(), size,
                                                         outcome.category(), outcome.message(),
                                                         outcome.rule()),
                        "the file could not be resized");
    }

    /// Writes a file whole. Buffered until commit.
    void write_file(const std::string& path, const std::uint8_t* bytes, std::size_t length) const
    {
        detail::Outcome outcome;
        outcome.require(remanence_filesystem_write_file(get(), path.c_str(), bytes, length,
                                                        outcome.category(), outcome.message(),
                                                        outcome.rule()),
                        "the file could not be written");
    }

    void write_file(const std::string& path, const std::vector<std::uint8_t>& bytes) const
    {
        write_file(path, bytes.data(), bytes.size());
    }

    /// Ensures a directory exists, missing parents included.
    void make_directory(const std::string& path) const
    {
        detail::Outcome outcome;
        outcome.require(remanence_filesystem_make_directory(get(), path.c_str(),
                                                            outcome.category(), outcome.message(),
                                                            outcome.rule()),
                        "the directory could not be made");
    }

    /// Every file under `path` gathered as a load's sources.
    FileSourceList files(const std::string& path = std::string{})
    {
        detail::Outcome outcome;
        RemanenceFileSourceList* gathered = remanence_space_files(
            get(), path.c_str(), outcome.category(), outcome.message(), outcome.rule());
        return FileSourceList(outcome.require(gathered, "the sources could not be gathered"));
    }
};

// ---------------------------------------------------------------------
// Partition — one entry of a medium's evidence pool
// ---------------------------------------------------------------------

/// What the scheme declared about one partition, what the library
/// composed over it, and the doors onto that composition.
///
/// It is a snapshot rather than a borrow: the facts are copied when the
/// pool answers, and every string reached through them dies with this
/// handle. Both doors may be opened off one partition; both compose the
/// same node.
class Partition : public detail::Held<RemanencePartition> {
public:
    using Held::Held;

    /// The scheme's own ordinal, which is how the pool names it.
    std::uint32_t ordinal() const noexcept { return remanence_partition_ordinal(get()); }

    /// Whether this is the direct partition — the library's own
    /// composition over a medium that records no scheme.
    bool is_direct() const noexcept { return remanence_partition_is_direct(get()); }

    bool active() const noexcept { return remanence_partition_active(get()); }

    /// The type byte the scheme recorded, absent where it recorded none.
    std::optional<std::uint8_t> type_byte() const noexcept
    {
        std::uint8_t value = 0;
        if (!remanence_partition_type_byte(get(), &value)) {
            return std::nullopt;
        }
        return value;
    }

    std::optional<std::string> type_reading() const
    {
        return detail::optional_copied(remanence_partition_type_reading(get()));
    }

    /// Whether composition claimed it.
    bool is_claimed() const noexcept { return remanence_partition_is_claimed(get()); }

    std::optional<std::string> placement() const
    {
        return detail::optional_copied(remanence_partition_placement(get()));
    }

    RegionRole role() const noexcept
    {
        return static_cast<RegionRole>(remanence_partition_role(get()));
    }

    std::optional<std::uint64_t> start_bytes() const noexcept
    {
        std::uint64_t value = 0;
        if (!remanence_partition_start_bytes(get(), &value)) {
            return std::nullopt;
        }
        return value;
    }

    std::optional<std::uint64_t> length_bytes() const noexcept
    {
        std::uint64_t value = 0;
        if (!remanence_partition_length_bytes(get(), &value)) {
            return std::nullopt;
        }
        return value;
    }

    /// Whether the addressable door answers here.
    bool is_addressable() const noexcept { return remanence_partition_is_addressable(get()); }

    /// Whether the namespace door does.
    bool bears_namespace() const noexcept { return remanence_partition_bears_namespace(get()); }

    /// The composed volume's identity, where one composed.
    std::optional<std::uint64_t> volume_id() const noexcept
    {
        std::uint64_t value = 0;
        if (!remanence_partition_volume_id(get(), &value)) {
            return std::nullopt;
        }
        return value;
    }

    std::optional<ErrorCategory> issue_category() const noexcept
    {
        RemanenceErrorCategory category = REMANENCE_ERROR_CATEGORY_IO;
        if (!remanence_partition_issue_category(get(), &category)) {
            return std::nullopt;
        }
        return static_cast<ErrorCategory>(category);
    }

    std::optional<std::string> issue() const
    {
        return detail::optional_copied(remanence_partition_issue(get()));
    }

    std::vector<std::string> evidence() const
    {
        const std::size_t count = remanence_partition_evidence_count(get());
        std::vector<std::string> lines;
        lines.reserve(count);
        for (std::size_t at = 0; at < count; at += 1) {
            lines.push_back(detail::copied(remanence_partition_evidence(get(), at)));
        }
        return lines;
    }

    /// Where this partition came from, where it is the library's own
    /// composition rather than a scheme's declaration.
    std::optional<std::string> provenance() const
    {
        return detail::optional_copied(remanence_partition_provenance(get()));
    }

    /// States the caller's reading of the recorded type and checks it.
    /// The refusal is the whole value: it throws where the reading
    /// disagrees, and where nothing was recorded to check against.
    void check_type(const std::string& type_id) const
    {
        detail::Outcome outcome;
        outcome.require(remanence_partition_check_type(get(), type_id.c_str(), outcome.category(),
                                                       outcome.message(), outcome.rule()),
                        "the partition is not of that type");
    }

    /// The addressable vantage.
    Volume volume() const
    {
        detail::Outcome outcome;
        RemanenceSpace* space = remanence_partition_volume(get(), outcome.category(),
                                                           outcome.message(), outcome.rule());
        return Volume(outcome.require(space, "no volume composed here"));
    }

    /// The namespace vantage, as the evidence recognized it.
    Filesystem filesystem() const
    {
        detail::Outcome outcome;
        RemanenceSpace* space = remanence_partition_filesystem(get(), outcome.category(),
                                                              outcome.message(), outcome.rule());
        return Filesystem(outcome.require(space, "no filesystem was recognized here"));
    }

    /// The namespace vantage under the caller's own declaration of what
    /// the filesystem is.
    Filesystem filesystem_as(const std::string& id) const
    {
        detail::Outcome outcome;
        RemanenceSpace* space = remanence_partition_filesystem_as(
            get(), id.c_str(), outcome.category(), outcome.message(), outcome.rule());
        return Filesystem(outcome.require(space, "that filesystem does not read here"));
    }
};

// ---------------------------------------------------------------------
// The flux presentations
//
// A recording read as the medium actually holds it, and the ladder that
// stands on it: the image, the family's hardware bitstream, the encoded
// bytestream above that, and the sectors the recording states for
// itself. Each rung is materialized from the one below under a declared
// bound, is a handle of its own, and leaves the rung beneath untouched.
//
// **The records here are the ABI's own, aliased rather than restated.**
// A half-track, a location, a claim, an orbit and a hole carry no
// strings and no ownership — they are plain numbers the ABI copies into
// an out-parameter — so a C++ struct beside them would add nothing but
// a place for the two to disagree.
// ---------------------------------------------------------------------

using P64HalfTrack = RemanenceP64HalfTrack;
using BitstreamLocation = RemanenceBitstreamLocation;
using BytestreamLocation = RemanenceBytestreamLocation;
using SectorLocation = RemanenceSectorLocation;
using SectorClaim = RemanenceSectorClaim;
using ContestedAddress = RemanenceContestedAddress;
using FluxHole = RemanenceFluxHole;
using FluxOrbit = RemanenceFluxOrbit;
using D64Block = RemanenceD64Block;
using G64HalfTrack = RemanenceG64HalfTrack;

/// One kind of loss a crossing does not carry.
///
/// Every rung of the ladder and every rendition off it accounts for what
/// it dropped, in the source's own terms: a count is not an account.
struct DeclaredLoss {
    /// The stable code for what was lost.
    std::string code;
    /// What was lost, in the source's own terms.
    std::string detail;
    /// How much of it there was, in whatever the detail counts.
    std::uint64_t amount;
};

namespace detail {

/// The declared-loss account, which every flux handle answers the same
/// way — the four ABI functions differ, the shape does not.
template <typename T, typename Count, typename Code, typename Detail, typename Amount>
std::vector<DeclaredLoss> losses(const T* handle, Count count, Code code, Detail detail,
                                 Amount amount)
{
    const std::size_t held = count(handle);
    std::vector<DeclaredLoss> account;
    account.reserve(held);
    for (std::size_t at = 0; at < held; at += 1) {
        account.push_back({copied(code(handle, at)), copied(detail(handle, at)),
                           amount(handle, at)});
    }
    return account;
}

/// The same for an evidence list, which they also all answer (P4).
template <typename T, typename Count, typename At>
std::vector<std::string> lines(const T* handle, Count count, At at_index)
{
    const std::size_t held = count(handle);
    std::vector<std::string> found;
    found.reserve(held);
    for (std::size_t at = 0; at < held; at += 1) {
        found.push_back(copied(at_index(handle, at)));
    }
    return found;
}

/// And the same for a record the ABI copies into an out-parameter.
template <typename Record, typename T, typename Count, typename At>
std::vector<Record> records(const T* handle, Count count, At at_index)
{
    const std::size_t held = count(handle);
    std::vector<Record> found;
    found.reserve(held);
    for (std::size_t at = 0; at < held; at += 1) {
        Record record{};
        if (!at_index(handle, at, &record)) {
            break;
        }
        found.push_back(record);
    }
    return found;
}

} // namespace detail

/// What a p64 container carried, or will carry, of one image.
class P64Report : public detail::Held<RemanenceP64Report> {
public:
    using Held::Held;

    /// The container format's stable identifier, `p64`.
    std::string format_id() const { return detail::copied(remanence_p64_format_id(get())); }

    std::string format_name() const { return detail::copied(remanence_p64_format_name(get())); }

    /// The container's declared format version.
    std::uint32_t version() const noexcept { return remanence_p64_version(get()); }

    bool write_protected() const noexcept { return remanence_p64_write_protected(get()); }

    bool double_sided() const noexcept { return remanence_p64_double_sided(get()); }

    /// The drive profile the container's own signature names, and the
    /// frame that profile declares.
    std::string profile_id() const { return detail::copied(remanence_p64_profile_id(get())); }

    std::uint64_t reference_clock_hz() const noexcept
    {
        return remanence_p64_reference_clock_hz(get());
    }

    std::uint64_t cycles_per_rotation() const noexcept
    {
        return remanence_p64_cycles_per_rotation(get());
    }

    std::size_t half_track_count() const noexcept { return remanence_p64_half_track_count(get()); }

    P64HalfTrack half_track(std::size_t index) const
    {
        detail::in_range(index, half_track_count(), "p64 half-track index");
        P64HalfTrack track{};
        remanence_p64_half_track(get(), index, &track);
        return track;
    }

    std::vector<P64HalfTrack> half_tracks() const
    {
        return detail::records<P64HalfTrack>(get(), remanence_p64_half_track_count,
                                             remanence_p64_half_track);
    }

    std::vector<DeclaredLoss> declared_losses() const
    {
        return detail::losses(get(), remanence_p64_declared_loss_count,
                              remanence_p64_declared_loss_code,
                              remanence_p64_declared_loss_detail,
                              remanence_p64_declared_loss_amount);
    }

    /// How the container was recognized and what this adapter claims of
    /// it.
    std::vector<std::string> evidence() const
    {
        return detail::lines(get(), remanence_p64_evidence_count, remanence_p64_evidence);
    }
};

/// What a g64 rendition carried, or will carry, of one image.
class G64Report : public detail::Held<RemanenceG64Report> {
public:
    using Held::Held;

    /// The artifact written, absent where nothing was written — a
    /// description rather than a rendition.
    std::optional<std::string> path() const
    {
        return detail::optional_copied(remanence_g64_report_path(get()));
    }

    std::uint64_t artifact_bytes() const noexcept
    {
        return remanence_g64_report_artifact_bytes(get());
    }

    std::size_t half_track_count() const noexcept
    {
        return remanence_g64_report_half_track_count(get());
    }

    G64HalfTrack half_track(std::size_t index) const
    {
        detail::in_range(index, half_track_count(), "g64 half-track index");
        G64HalfTrack track{};
        remanence_g64_report_half_track(get(), index, &track);
        return track;
    }

    std::vector<G64HalfTrack> half_tracks() const
    {
        return detail::records<G64HalfTrack>(get(), remanence_g64_report_half_track_count,
                                             remanence_g64_report_half_track);
    }

    std::vector<DeclaredLoss> declared_losses() const
    {
        return detail::losses(get(), remanence_g64_report_declared_loss_count,
                              remanence_g64_report_declared_loss_code,
                              remanence_g64_report_declared_loss_detail,
                              remanence_g64_report_declared_loss_amount);
    }
};

/// What a d64 rendition carried, or will carry, of one image.
class D64Report : public detail::Held<RemanenceD64Report> {
public:
    using Held::Held;

    std::optional<std::string> path() const
    {
        return detail::optional_copied(remanence_d64_report_path(get()));
    }

    std::uint64_t artifact_bytes() const noexcept
    {
        return remanence_d64_report_artifact_bytes(get());
    }

    /// How many blocks the recording answered, against how many the
    /// rendition's fixed shape defines.
    std::uint32_t blocks_read() const noexcept { return remanence_d64_report_blocks_read(get()); }

    std::uint32_t blocks_defined() const noexcept
    {
        return remanence_d64_report_blocks_defined(get());
    }

    std::uint32_t failed_checksums() const noexcept
    {
        return remanence_d64_report_failed_checksums(get());
    }

    /// The addresses the rendition defines and the recording did not
    /// answer.
    std::vector<D64Block> missing() const
    {
        return detail::records<D64Block>(get(), remanence_d64_report_missing_count,
                                         remanence_d64_report_missing);
    }

    std::vector<DeclaredLoss> declared_losses() const
    {
        return detail::losses(get(), remanence_d64_report_declared_loss_count,
                              remanence_d64_report_declared_loss_code,
                              remanence_d64_report_declared_loss_detail,
                              remanence_d64_report_declared_loss_amount);
    }
};

/// What writing an image into a `.remanence` artifact carried.
class FluxWriteReport : public detail::Held<RemanenceFluxWriteReport> {
public:
    using Held::Held;

    std::optional<std::string> path() const
    {
        return detail::optional_copied(remanence_flux_write_report_path(get()));
    }

    std::uint64_t artifact_bytes() const noexcept
    {
        return remanence_flux_write_report_artifact_bytes(get());
    }

    std::uint64_t orbits() const noexcept { return remanence_flux_write_report_orbits(get()); }

    std::uint64_t points() const noexcept { return remanence_flux_write_report_points(get()); }

    std::vector<DeclaredLoss> declared_losses() const
    {
        return detail::losses(get(), remanence_flux_write_report_declared_loss_count,
                              remanence_flux_write_report_declared_loss_code,
                              remanence_flux_write_report_declared_loss_detail,
                              remanence_flux_write_report_declared_loss_amount);
    }
};

/// The recording's own sectors, read by the address the recording states
/// for them.
///
/// It answers only where the recording is unambiguous — one readable
/// claim, or several holding the same bytes. Every other outcome is a
/// refusal naming its rule; nothing is repaired and no block is filled
/// in.
class C1541Sectors : public detail::Held<RemanenceC1541Sectors> {
public:
    using Held::Held;

    /// How long a payload one sector holds, which is what `read` needs.
    std::uint32_t payload_bytes() const noexcept
    {
        return remanence_c1541_sectors_payload_bytes(get());
    }

    /// Reads one sector by the address the recording states for it.
    void read(std::uint8_t track, std::uint8_t sector, std::uint8_t* buffer,
              std::size_t length) const
    {
        detail::Outcome outcome;
        outcome.require(remanence_c1541_sectors_read(get(), track, sector, buffer, length,
                                                     outcome.category(), outcome.message(),
                                                     outcome.rule()),
                        "that sector does not read");
    }

    std::vector<std::uint8_t> read(std::uint8_t track, std::uint8_t sector) const
    {
        std::vector<std::uint8_t> payload(payload_bytes());
        if (!payload.empty()) {
            read(track, sector, payload.data(), payload.size());
        }
        return payload;
    }

    /// The direct partition over this recording — the library's own
    /// composition of the whole content, which is how a namespace above
    /// it is reached (P19).
    std::optional<Partition> partition() const
    {
        RemanencePartition* composed = remanence_c1541_sectors_partition(get());
        if (composed == nullptr) {
            return std::nullopt;
        }
        return Partition(composed);
    }

    std::string profile_id() const
    {
        return detail::copied(remanence_c1541_sectors_profile_id(get()));
    }

    /// The record grammar the recognition ran under.
    std::string grammar_id() const
    {
        return detail::copied(remanence_c1541_sectors_grammar_id(get()));
    }

    std::string grammar_name() const
    {
        return detail::copied(remanence_c1541_sectors_grammar_name(get()));
    }

    /// The private session storage this layer occupies, and how much of
    /// it is resident — the points are never held whole (P27).
    std::uint64_t backing_bytes() const noexcept
    {
        return remanence_c1541_sectors_backing_bytes(get());
    }

    std::uint64_t resident_bytes() const noexcept
    {
        return remanence_c1541_sectors_resident_bytes(get());
    }

    std::size_t location_count() const noexcept
    {
        return remanence_c1541_sectors_location_count(get());
    }

    SectorLocation location(std::size_t index) const
    {
        detail::in_range(index, location_count(), "sector location index");
        SectorLocation found{};
        remanence_c1541_sectors_location(get(), index, &found);
        return found;
    }

    std::vector<SectorLocation> locations() const
    {
        return detail::records<SectorLocation>(get(), remanence_c1541_sectors_location_count,
                                               remanence_c1541_sectors_location);
    }

    std::size_t claim_count() const noexcept { return remanence_c1541_sectors_claim_count(get()); }

    /// One record the recognition read, with the evidence for every
    /// claim it makes.
    SectorClaim claim(std::size_t index) const
    {
        detail::in_range(index, claim_count(), "sector claim index");
        SectorClaim found{};
        remanence_c1541_sectors_claim(get(), index, &found);
        return found;
    }

    std::vector<SectorClaim> claims() const
    {
        return detail::records<SectorClaim>(get(), remanence_c1541_sectors_claim_count,
                                            remanence_c1541_sectors_claim);
    }

    /// Which rule of the sector-layer set stands in the way of this
    /// claim. **Empty for a claim that reads**, which is the ABI's own
    /// spelling of "no rule was broken".
    std::string claim_rule(std::size_t index) const
    {
        detail::in_range(index, claim_count(), "sector claim index");
        return detail::copied(remanence_c1541_sectors_claim_rule(get(), index));
    }

    /// Why this claim does not read, in the layer's own terms; empty for
    /// one that does.
    std::string claim_refusal(std::size_t index) const
    {
        detail::in_range(index, claim_count(), "sector claim index");
        return detail::copied(remanence_c1541_sectors_claim_refusal(get(), index));
    }

    /// The addresses more than one readable claim states.
    std::vector<ContestedAddress> contested() const
    {
        return detail::records<ContestedAddress>(get(), remanence_c1541_sectors_contested_count,
                                                 remanence_c1541_sectors_contested);
    }

    std::vector<DeclaredLoss> declared_losses() const
    {
        return detail::losses(get(), remanence_c1541_sectors_declared_loss_count,
                              remanence_c1541_sectors_declared_loss_code,
                              remanence_c1541_sectors_declared_loss_detail,
                              remanence_c1541_sectors_declared_loss_amount);
    }

    /// The grammar and policy that produced it, and everything the
    /// bytestream said beneath it, in that order.
    std::vector<std::string> evidence() const
    {
        return detail::lines(get(), remanence_c1541_sectors_evidence_count,
                             remanence_c1541_sectors_evidence);
    }
};

/// The family's encoded bytestream: the bytes a group code resolved out
/// of the channel, before anything assigns them a meaning.
///
/// No byte here is a header, a sector or a file — the layers that decide
/// that sit above.
class C1541Bytestream : public detail::Held<RemanenceC1541Bytestream> {
public:
    using Held::Held;

    std::string profile_id() const
    {
        return detail::copied(remanence_c1541_bytestream_profile_id(get()));
    }

    /// The group code the stream was resolved under, and its shape.
    std::string codec_id() const
    {
        return detail::copied(remanence_c1541_bytestream_codec_id(get()));
    }

    std::string codec_name() const
    {
        return detail::copied(remanence_c1541_bytestream_codec_name(get()));
    }

    std::uint32_t symbol_bits() const noexcept
    {
        return remanence_c1541_bytestream_symbol_bits(get());
    }

    std::uint32_t data_bits() const noexcept
    {
        return remanence_c1541_bytestream_data_bits(get());
    }

    std::uint32_t symbols_per_byte() const noexcept
    {
        return remanence_c1541_bytestream_symbols_per_byte(get());
    }

    std::uint64_t backing_bytes() const noexcept
    {
        return remanence_c1541_bytestream_backing_bytes(get());
    }

    std::uint64_t resident_bytes() const noexcept
    {
        return remanence_c1541_bytestream_resident_bytes(get());
    }

    std::size_t location_count() const noexcept
    {
        return remanence_c1541_bytestream_location_count(get());
    }

    BytestreamLocation location(std::size_t index) const
    {
        detail::in_range(index, location_count(), "bytestream location index");
        BytestreamLocation found{};
        remanence_c1541_bytestream_location(get(), index, &found);
        return found;
    }

    std::vector<BytestreamLocation> locations() const
    {
        return detail::records<BytestreamLocation>(get(),
                                                   remanence_c1541_bytestream_location_count,
                                                   remanence_c1541_bytestream_location);
    }

    /// How many framed bytes one location holds, addressed in the
    /// family's own terms — the 1541 numbers its tracks from 1. A track
    /// the stream does not hold is refused naming what it does hold.
    std::uint64_t location_bytes(std::uint32_t track) const
    {
        detail::Outcome outcome;
        std::uint64_t bytes = 0;
        outcome.require(remanence_c1541_bytestream_location_bytes(get(), track, &bytes,
                                                                  outcome.category(),
                                                                  outcome.message(),
                                                                  outcome.rule()),
                        "the stream holds no such track");
        return bytes;
    }

    /// Reads framed bytes of one track, whole or not at all. Bytes
    /// number from the first framed byte, because nothing before sync is
    /// a byte at all.
    void location_read_at(std::uint32_t track, std::uint64_t offset, std::uint8_t* buffer,
                          std::size_t length) const
    {
        detail::Outcome outcome;
        outcome.require(remanence_c1541_bytestream_location_read_at(get(), track, offset, buffer,
                                                                    length, outcome.category(),
                                                                    outcome.message(),
                                                                    outcome.rule()),
                        "those bytes do not read");
    }

    std::vector<std::uint8_t> location_read_at(std::uint32_t track, std::uint64_t offset,
                                               std::size_t length) const
    {
        std::vector<std::uint8_t> buffer(length);
        if (length > 0) {
            location_read_at(track, offset, buffer.data(), length);
        }
        return buffer;
    }

    /// Recognizes the recording's own sectors under the family's
    /// declared record grammar. `cache_bytes` is the working-set bound
    /// (P27); the bytestream is untouched.
    C1541Sectors recognize_sectors(std::uint64_t cache_bytes = 0) const
    {
        detail::Outcome outcome;
        RemanenceC1541Sectors* recognized = remanence_c1541_bytestream_recognize_sectors(
            get(), cache_bytes, outcome.category(), outcome.message(), outcome.rule());
        return C1541Sectors(outcome.require(recognized, "no sectors were recognized"));
    }

    std::vector<DeclaredLoss> declared_losses() const
    {
        return detail::losses(get(), remanence_c1541_bytestream_declared_loss_count,
                              remanence_c1541_bytestream_declared_loss_code,
                              remanence_c1541_bytestream_declared_loss_detail,
                              remanence_c1541_bytestream_declared_loss_amount);
    }

    /// The codec, the channel beneath it and the medium policy beneath
    /// that, in that order.
    std::vector<std::string> evidence() const
    {
        return detail::lines(get(), remanence_c1541_bytestream_evidence_count,
                             remanence_c1541_bytestream_evidence);
    }
};

/// The family's hardware bitstream: what the read channel resolved out
/// of the recording, under the profile's declared mechanics.
///
/// **Two doors mint it, and their lifetimes differ.**
/// `FluxImage::materialize_c1541_bitstream` gives a handle that owns the
/// stream it materialized. `Medium::bitstream` gives a *view* of the
/// stream cached in the pooled medium: it stops answering once the
/// medium is released and **must not outlive its session**. Destroying
/// either is correct — the view discards only itself — but the second
/// is a borrow this class cannot enforce, exactly as the ABI cannot.
class C1541Bitstream : public detail::Held<RemanenceC1541Bitstream> {
public:
    using Held::Held;

    /// The drive profile the stream was resolved under, and the frame it
    /// declares.
    std::string profile_id() const
    {
        return detail::copied(remanence_c1541_bitstream_profile_id(get()));
    }

    std::string profile_name() const
    {
        return detail::copied(remanence_c1541_bitstream_profile_name(get()));
    }

    std::uint32_t profile_version() const noexcept
    {
        return remanence_c1541_bitstream_profile_version(get());
    }

    std::uint64_t reference_clock_hz() const noexcept
    {
        return remanence_c1541_bitstream_reference_clock_hz(get());
    }

    std::uint64_t cycles_per_rotation() const noexcept
    {
        return remanence_c1541_bitstream_cycles_per_rotation(get());
    }

    std::uint64_t backing_bytes() const noexcept
    {
        return remanence_c1541_bitstream_backing_bytes(get());
    }

    std::uint64_t resident_bytes() const noexcept
    {
        return remanence_c1541_bitstream_resident_bytes(get());
    }

    std::size_t location_count() const noexcept
    {
        return remanence_c1541_bitstream_location_count(get());
    }

    /// One location the stream holds, and what the channel resolved
    /// there.
    BitstreamLocation location(std::size_t index) const
    {
        detail::in_range(index, location_count(), "bitstream location index");
        BitstreamLocation found{};
        remanence_c1541_bitstream_location(get(), index, &found);
        return found;
    }

    std::vector<BitstreamLocation> locations() const
    {
        return detail::records<BitstreamLocation>(get(), remanence_c1541_bitstream_location_count,
                                                  remanence_c1541_bitstream_location);
    }

    /// Materializes the encoded bytestream above this one under its
    /// declared group code — no policy to pass, because the type carries
    /// one. The bitstream is untouched.
    C1541Bytestream materialize_bytestream(std::uint64_t cache_bytes = 0) const
    {
        detail::Outcome outcome;
        RemanenceC1541Bytestream* stream = remanence_c1541_bitstream_materialize_bytestream(
            get(), cache_bytes, outcome.category(), outcome.message(), outcome.rule());
        return C1541Bytestream(outcome.require(stream, "no bytestream materialized"));
    }

    std::vector<DeclaredLoss> declared_losses() const
    {
        return detail::losses(get(), remanence_c1541_bitstream_declared_loss_count,
                              remanence_c1541_bitstream_declared_loss_code,
                              remanence_c1541_bitstream_declared_loss_detail,
                              remanence_c1541_bitstream_declared_loss_amount);
    }

    std::vector<std::string> evidence() const
    {
        return detail::lines(get(), remanence_c1541_bitstream_evidence_count,
                             remanence_c1541_bitstream_evidence);
    }
};

/// An opened `.remanence` artifact: the claim on the file, and the
/// points it decoded into private session storage.
///
/// There is no device to load a flux artifact into — it is read through
/// its own type, which is why this stands beside the session rather than
/// inside it.
class FluxImage : public detail::Held<RemanenceFluxImage> {
public:
    using Held::Held;

    /// Opens the artifact at `path`, claiming the file and decoding the
    /// image once. The magic, the sentinel and the layout version are
    /// checked before anything else is believed.
    ///
    /// This is a named constructor rather than a free function because
    /// the ABI spells it that way — `remanence_flux_image_open` names
    /// its type first, where `remanence_discover_media` names none.
    static FluxImage open(const std::string& path)
    {
        detail::Outcome outcome;
        RemanenceFluxImage* image = remanence_flux_image_open(
            path.c_str(), outcome.category(), outcome.message(), outcome.rule());
        return FluxImage(outcome.require(image, "the image could not be opened"));
    }

    /// The same under a declared cache bound: at most `cache_bytes` of
    /// the decoded image stays resident. The bound narrows the working
    /// set; it never refuses service (P27).
    static FluxImage open(const std::string& path, std::uint64_t cache_bytes)
    {
        detail::Outcome outcome;
        RemanenceFluxImage* image = remanence_flux_image_open_with_cache(
            path.c_str(), cache_bytes, outcome.category(), outcome.message(), outcome.rule());
        return FluxImage(outcome.require(image, "the image could not be opened"));
    }

    std::optional<std::string> path() const
    {
        return detail::optional_copied(remanence_flux_image_path(get()));
    }

    /// The artifact format's stable identifier, `remanence`.
    std::string format_id() const { return detail::copied(remanence_flux_image_format_id(get())); }

    std::string format_name() const
    {
        return detail::copied(remanence_flux_image_format_name(get()));
    }

    /// Which P7 mode the open obtained on the artifact.
    AccessMode access_mode() const noexcept
    {
        return static_cast<AccessMode>(remanence_flux_image_access_mode(get()));
    }

    /// The medium's shape in the model's own spelling — `5.25-inch`.
    std::string form_factor() const
    {
        return detail::copied(remanence_flux_image_form_factor(get()));
    }

    /// The angular unit every angle in the image is stated over — a unit
    /// rather than a measurement, so equality is exact.
    std::uint64_t angular_divisions() const noexcept
    {
        return remanence_flux_image_angular_divisions(get());
    }

    std::uint64_t backing_bytes() const noexcept
    {
        return remanence_flux_image_backing_bytes(get());
    }

    /// How much of that backing is resident. The points are never held
    /// whole.
    std::uint64_t resident_bytes() const noexcept
    {
        return remanence_flux_image_resident_bytes(get());
    }

    std::vector<FluxHole> holes() const
    {
        return detail::records<FluxHole>(get(), remanence_flux_image_hole_count,
                                         remanence_flux_image_hole);
    }

    std::vector<std::uint64_t> surfaces() const
    {
        return detail::records<std::uint64_t>(get(), remanence_flux_image_surface_count,
                                              remanence_flux_image_surface);
    }

    /// One orbit's identity and shape — never its points.
    std::vector<FluxOrbit> orbits() const
    {
        return detail::records<FluxOrbit>(get(), remanence_flux_image_orbit_count,
                                          remanence_flux_image_orbit);
    }

    /// How the image came to be known, in human-readable terms.
    std::vector<std::string> provenance() const
    {
        return detail::lines(get(), remanence_flux_image_provenance_count,
                             remanence_flux_image_provenance);
    }

    /// Materializes the family's hardware bitstream from what the image
    /// holds. It takes no policy, because the type carries one (P30);
    /// `cache_bytes` is the working-set bound. The image is untouched.
    C1541Bitstream materialize_c1541_bitstream(std::uint64_t cache_bytes = 0) const
    {
        detail::Outcome outcome;
        RemanenceC1541Bitstream* stream = remanence_flux_image_materialize_c1541_bitstream(
            get(), cache_bytes, outcome.category(), outcome.message(), outcome.rule());
        return C1541Bitstream(outcome.require(stream, "no bitstream materialized"));
    }

    /// Writes the image into a new `.remanence` artifact. An existing
    /// destination is a named refusal rather than an overwrite, and an
    /// interruption leaves the destination absent rather than half an
    /// artifact.
    FluxWriteReport write(const std::string& path) const
    {
        detail::Outcome outcome;
        RemanenceFluxWriteReport* report = remanence_flux_image_write(
            get(), path.c_str(), outcome.category(), outcome.message(), outcome.rule());
        return FluxWriteReport(outcome.require(report, "the image could not be written"));
    }

    /// What a d64 rendition would carry, without writing one.
    D64Report describe_d64() const
    {
        detail::Outcome outcome;
        RemanenceD64Report* report = remanence_flux_image_describe_d64(
            get(), outcome.category(), outcome.message(), outcome.rule());
        return D64Report(outcome.require(report, "no d64 rendition describes this image"));
    }

    D64Report write_d64(const std::string& path) const
    {
        detail::Outcome outcome;
        RemanenceD64Report* report = remanence_flux_image_write_d64(
            get(), path.c_str(), outcome.category(), outcome.message(), outcome.rule());
        return D64Report(outcome.require(report, "the d64 rendition could not be written"));
    }

    G64Report describe_g64() const
    {
        detail::Outcome outcome;
        RemanenceG64Report* report = remanence_flux_image_describe_g64(
            get(), outcome.category(), outcome.message(), outcome.rule());
        return G64Report(outcome.require(report, "no g64 rendition describes this image"));
    }

    G64Report write_g64(const std::string& path) const
    {
        detail::Outcome outcome;
        RemanenceG64Report* report = remanence_flux_image_write_g64(
            get(), path.c_str(), outcome.category(), outcome.message(), outcome.rule());
        return G64Report(outcome.require(report, "the g64 rendition could not be written"));
    }

    P64Report describe_p64() const
    {
        detail::Outcome outcome;
        RemanenceP64Report* report = remanence_flux_image_describe_p64(
            get(), outcome.category(), outcome.message(), outcome.rule());
        return P64Report(outcome.require(report, "no p64 container describes this image"));
    }

    P64Report write_p64(const std::string& path) const
    {
        detail::Outcome outcome;
        RemanenceP64Report* report = remanence_flux_image_write_p64(
            get(), path.c_str(), outcome.category(), outcome.message(), outcome.rule());
        return P64Report(outcome.require(report, "the p64 container could not be written"));
    }
};

// ---------------------------------------------------------------------
// Medium — the content handle, and where every content verb lives
// ---------------------------------------------------------------------

/// One medium in a session's media pool.
///
/// **A view, not an owner.** The session owns the medium and this holds
/// no lifetime of its own — which is why it copies freely and has no
/// destructor to speak of. It stays valid until the medium is released
/// or the session dies, and it names the medium by session and pool
/// identity rather than by pointer, so a later load can never make it
/// point at a stranger.
class Medium {
public:
    explicit Medium(RemanenceMedium* borrowed) noexcept : handle_(borrowed) {}

    /// The borrowed handle, for any C function this header does not
    /// wrap.
    RemanenceMedium* get() const noexcept { return handle_; }

    /// This medium's identity in its session's pool.
    std::uint64_t id() const noexcept { return remanence_medium_id(handle_); }

    /// Whether a device currently links it. Unlinked is ordinary: it is
    /// loaded, claimed, and answering.
    bool is_linked() const noexcept { return remanence_medium_is_linked(handle_); }

    /// The article this medium is (P14) — the physical substrate.
    std::optional<std::string> article() const
    {
        return detail::optional_copied(remanence_medium_article(handle_));
    }

    /// What recorded its content, absent where nothing did — an
    /// archive's honest answer, and an authored blank's.
    std::optional<std::string> device_type() const
    {
        return detail::optional_copied(remanence_medium_device_type(handle_));
    }

    /// The artifact it was loaded from, absent where the caller's handle
    /// has no recoverable name.
    std::optional<std::string> path() const
    {
        return detail::optional_copied(remanence_medium_path(handle_));
    }

    std::optional<std::string> image_path() const
    {
        return detail::optional_copied(remanence_medium_image_path(handle_));
    }

    std::uint64_t image_size_bytes() const noexcept
    {
        return remanence_medium_image_size_bytes(handle_);
    }

    /// The presented disk's size in bytes.
    std::uint64_t size() const noexcept { return remanence_medium_size(handle_); }

    /// Whether anything is buffered and uncommitted.
    bool is_modified() const noexcept { return remanence_medium_is_modified(handle_); }

    AccessMode mode() const noexcept
    {
        return static_cast<AccessMode>(remanence_medium_mode(handle_));
    }

    /// The image container format, absent for a medium that is no disk
    /// image.
    std::optional<DiskFormat> format() const noexcept
    {
        RemanenceDiskFormat found = REMANENCE_DISK_FORMAT_RAW;
        if (!remanence_medium_format(handle_, &found)) {
            return std::nullopt;
        }
        return static_cast<DiskFormat>(found);
    }

    /// The qcow2 version, where the medium is one.
    std::uint32_t qcow2_version() const noexcept
    {
        return remanence_medium_qcow2_version(handle_);
    }

    std::uint32_t vdi_version_major() const noexcept
    {
        return remanence_medium_vdi_version_major(handle_);
    }

    std::uint32_t vdi_version_minor() const noexcept
    {
        return remanence_medium_vdi_version_minor(handle_);
    }

    /// Reads the resolved image's raw plane — the bounded access form:
    /// the image streams from its backing and is never resident whole.
    void read_at(std::uint64_t offset, std::uint8_t* buffer, std::size_t length) const
    {
        detail::Outcome outcome;
        outcome.require(remanence_medium_read_at(handle_, offset, buffer, length,
                                                 outcome.category(), outcome.message(),
                                                 outcome.rule()),
                        "the image could not be read");
    }

    std::vector<std::uint8_t> read_at(std::uint64_t offset, std::size_t length) const
    {
        std::vector<std::uint8_t> buffer(length);
        if (length > 0) {
            read_at(offset, buffer.data(), length);
        }
        return buffer;
    }

    /// The artifact's nesting layers and probable filesystem.
    Identification identify() const
    {
        return Identification(remanence_medium_identify(handle_));
    }

    /// What the open established about the evidence beneath it (P28).
    Assurance assurance() const { return Assurance(remanence_medium_assurance(handle_)); }

    /// The recording's own coordinates, with every reading that produced
    /// them.
    Geometry geometry() const { return Geometry(remanence_medium_geometry(handle_)); }

    /// Reads one sector in the recording's own coordinates. Sectors
    /// number from one.
    void read_sector(std::uint32_t cylinder, std::uint32_t head, std::uint32_t sector,
                     std::uint8_t* buffer, std::size_t length)
    {
        detail::Outcome outcome;
        outcome.require(remanence_medium_read_sector(handle_, cylinder, head, sector, buffer,
                                                     length, outcome.category(), outcome.message(),
                                                     outcome.rule()),
                        "the sector could not be read");
    }

    std::vector<std::uint8_t> read_sector(std::uint32_t cylinder, std::uint32_t head,
                                          std::uint32_t sector, std::size_t length)
    {
        std::vector<std::uint8_t> buffer(length);
        if (length > 0) {
            read_sector(cylinder, head, sector, buffer.data(), length);
        }
        return buffer;
    }

    /// Writes one sector. Buffered until `commit` (P2).
    void write_sector(std::uint32_t cylinder, std::uint32_t head, std::uint32_t sector,
                      const std::uint8_t* data, std::size_t length)
    {
        detail::Outcome outcome;
        outcome.require(remanence_medium_write_sector(handle_, cylinder, head, sector, data, length,
                                                      outcome.category(), outcome.message(),
                                                      outcome.rule()),
                        "the sector could not be written");
    }

    void write_sector(std::uint32_t cylinder, std::uint32_t head, std::uint32_t sector,
                      const std::vector<std::uint8_t>& data)
    {
        write_sector(cylinder, head, sector, data.data(), data.size());
    }

    /// The scheme this medium's content is laid out under, absent where
    /// it records none — in which case the direct partition is the whole
    /// of it.
    std::optional<std::string> partition_scheme() const
    {
        return detail::optional_copied(remanence_medium_partition_scheme(handle_));
    }

    std::size_t partition_count() const noexcept
    {
        return remanence_medium_partition_count(handle_);
    }

    /// The scheme's own ordinal for the `index`th entry of the pool.
    std::uint32_t partition_ordinal(std::size_t index) const
    {
        detail::in_range(index, partition_count(), "partition pool index");
        std::uint32_t ordinal = 0;
        remanence_medium_partition_ordinal(handle_, index, &ordinal);
        return ordinal;
    }

    /// Every ordinal the pool holds, in pool order.
    std::vector<std::uint32_t> partition_ordinals() const
    {
        const std::size_t count = partition_count();
        std::vector<std::uint32_t> ordinals;
        ordinals.reserve(count);
        for (std::size_t at = 0; at < count; at += 1) {
            std::uint32_t ordinal = 0;
            remanence_medium_partition_ordinal(handle_, at, &ordinal);
            ordinals.push_back(ordinal);
        }
        return ordinals;
    }

    /// One partition by the scheme's own ordinal, absent where the pool
    /// holds no such entry.
    std::optional<Partition> partition(std::uint32_t ordinal)
    {
        RemanencePartition* found = remanence_medium_partition(handle_, ordinal);
        if (found == nullptr) {
            return std::nullopt;
        }
        return Partition(found);
    }

    /// One disk's layered inspection, as a snapshot.
    DiskReport inspect()
    {
        detail::Outcome outcome;
        RemanenceDiskReport* report =
            remanence_medium_inspect(handle_, outcome.category(), outcome.message(), outcome.rule());
        return DiskReport(outcome.require(report, "the medium could not be inspected"));
    }

    /// The commit point (P2, P9): everything buffered reaches the image,
    /// durably. Until this call nothing has touched the file.
    void commit()
    {
        detail::Outcome outcome;
        outcome.require(remanence_medium_commit(handle_, outcome.category(), outcome.message(),
                                                outcome.rule()),
                        "the commit failed");
    }

    /// Discards everything buffered; the image is untouched.
    void rollback() noexcept { remanence_medium_rollback(handle_); }

    /// The family's hardware bitstream over this medium's recording,
    /// materialized once into the pooled medium and answered from then
    /// on. It answers where the device type's profile bears flux, and
    /// refuses by name everywhere else — a block medium's recording is
    /// presented by its format adapter, and the two families are
    /// disjoint (P13).
    ///
    /// **The handle is a view of the pooled stream**, not an owner: it
    /// stops answering once the medium is released and must not outlive
    /// the session. Destroying it discards the view alone.
    C1541Bitstream bitstream()
    {
        detail::Outcome outcome;
        RemanenceC1541Bitstream* stream = remanence_medium_bitstream(
            handle_, outcome.category(), outcome.message(), outcome.rule());
        return C1541Bitstream(outcome.require(stream, "this medium bears no bitstream"));
    }

    /// The encoded bytestream above it, on the same terms.
    C1541Bytestream bytestream()
    {
        detail::Outcome outcome;
        RemanenceC1541Bytestream* stream = remanence_medium_bytestream(
            handle_, outcome.category(), outcome.message(), outcome.rule());
        return C1541Bytestream(outcome.require(stream, "this medium bears no bytestream"));
    }

private:
    RemanenceMedium* handle_;
};

// ---------------------------------------------------------------------
// StorageDevice — the slot, typed by the device that fills it
// ---------------------------------------------------------------------

/// One storage device of a session: the slot, what it is, and the state
/// of the medium in it.
///
/// **A view, as `Medium` is.** The session owns it, and it names the
/// device by session and attachment identity rather than by
/// pointer, so a later attach cannot leave it dangling.
class StorageDevice {
public:
    explicit StorageDevice(RemanenceDevice* borrowed) noexcept : handle_(borrowed) {}

    RemanenceDevice* get() const noexcept { return handle_; }

    /// This device's attachment identity — `hdd0` and the like.
    std::string attachment() const
    {
        return detail::copied(remanence_device_attachment(handle_));
    }

    /// What it is, by stable spelling — a device type's own, or
    /// `archive`.
    std::string slot() const
    {
        return detail::copied(remanence_device_slot(handle_));
    }

    /// The recording device type this slot is typed by, absent for the
    /// archive receiver, which records nothing.
    std::optional<std::string> device_type() const
    {
        return detail::optional_copied(remanence_device_type(handle_));
    }

    bool is_occupied() const noexcept { return remanence_device_is_occupied(handle_); }

    /// The identity of the medium in the slot, read beside
    /// `is_occupied`, which tells an empty slot from an identity of zero.
    std::uint64_t media_id() const noexcept { return remanence_device_media_id(handle_); }

    /// The medium in the slot, absent while it is empty.
    std::optional<Medium> medium() const
    {
        RemanenceMedium* found = remanence_device_medium(handle_);
        if (found == nullptr) {
            return std::nullopt;
        }
        return Medium(found);
    }

    /// Links the pooled medium into this slot. The check is device-type
    /// equality (P14): a medium belonging in another drive is refused
    /// naming both sides.
    void insert(std::uint64_t media_id)
    {
        detail::Outcome outcome;
        outcome.require(remanence_device_insert(handle_, media_id, outcome.category(),
                                                outcome.message(), outcome.rule()),
                        "the medium could not be inserted");
    }

    /// Severs the link and nothing more: the device stays in the
    /// session, the medium stays in the pool with everything buffered
    /// intact. Ejecting is not a commit point.
    void eject()
    {
        detail::Outcome outcome;
        outcome.require(
            remanence_device_eject(handle_, outcome.category(), outcome.message(), outcome.rule()),
            "the slot was already empty");
    }

private:
    RemanenceDevice* handle_;
};

// ---------------------------------------------------------------------
// Session — the claim and cache scope, its devices and its media (P32)
// ---------------------------------------------------------------------

/// An open session.
///
/// It owns the **media pool** (state) and the **devices**
/// (configuration) independently of each other, and it is the one thing
/// here whose destructor ends a claim: freeing it releases every medium,
/// closes every handle it took ownership of, and invalidates every view
/// taken from it.
class Session : public detail::Held<RemanenceSession> {
public:
    Session() : Held(remanence_session_new())
    {
        if (get() == nullptr) {
            throw std::bad_alloc();
        }
    }

    explicit Session(RemanenceSession* adopted) noexcept : Held(adopted) {}

    // --- the media pool: authorship, loads, lookups, release

    /// Creates blank media whole — authorship, the third fact class.
    ///
    /// `kind` is a stable spelling from `new_media_kinds()`. The
    /// coordinates are the author's own, for the kind whose claim takes
    /// them; every other kind takes zeros and refuses anything else by
    /// name. An authored blank assumes no device.
    Medium new_media(const std::string& kind, std::uint32_t cylinders = 0,
                     std::uint32_t heads = 0, std::uint32_t sectors_per_track = 0,
                     std::uint64_t sector_bytes = 0)
    {
        detail::Outcome outcome;
        RemanenceMedium* made = remanence_session_new_media(
            get(), kind.c_str(), cylinders, heads, sectors_per_track, sector_bytes,
            outcome.category(), outcome.message(), outcome.rule());
        return Medium(outcome.require(made, "the medium could not be authored"));
    }

    /// Loads the caller's own opened artifact as the format they declare
    /// it to be. **The library takes ownership of `source`**: closing it
    /// is the library's, at release or at session end (P7).
    Medium load_media(NativeHandle source, const std::string& format,
                      const std::optional<std::string>& device_type = std::nullopt,
                      std::uint64_t block_bytes = 0)
    {
        detail::Outcome outcome;
        RemanenceMedium* loaded = remanence_session_load_media(
            get(), source, format.c_str(), detail::pointer(device_type), block_bytes,
            outcome.category(), outcome.message(), outcome.rule());
        return Medium(outcome.require(loaded, "the artifact could not be loaded"));
    }

    /// The collection shape of the load, which only a format whose claim
    /// takes one reads. Every member is checked before any is adopted;
    /// once checked, the library owns all of them whatever the outcome.
    Medium load_media_collection(const std::vector<NativeHandle>& sources,
                                 const std::string& format,
                                 const std::optional<std::string>& device_type = std::nullopt,
                                 std::uint64_t block_bytes = 0)
    {
        detail::Outcome outcome;
        RemanenceMedium* loaded = remanence_session_load_media_collection(
            get(), sources.data(), sources.size(), format.c_str(), detail::pointer(device_type),
            block_bytes, outcome.category(), outcome.message(), outcome.rule());
        return Medium(outcome.require(loaded, "the collection could not be loaded"));
    }

    /// Loads one file taken from an archive medium's namespace. The
    /// source is consumed.
    Medium load_media_source(FileSource source, const std::string& format,
                             const std::optional<std::string>& device_type = std::nullopt,
                             std::uint64_t block_bytes = 0)
    {
        detail::Outcome outcome;
        RemanenceMedium* loaded = remanence_session_load_media_source(
            get(), source.release(), format.c_str(), detail::pointer(device_type), block_bytes,
            outcome.category(), outcome.message(), outcome.rule());
        return Medium(outcome.require(loaded, "the source could not be loaded"));
    }

    /// Loads a whole gathering of them. The list is consumed.
    Medium load_media_sources(FileSourceList sources, const std::string& format,
                              const std::optional<std::string>& device_type = std::nullopt,
                              std::uint64_t block_bytes = 0)
    {
        detail::Outcome outcome;
        RemanenceMedium* loaded = remanence_session_load_media_sources(
            get(), sources.release(), format.c_str(), detail::pointer(device_type), block_bytes,
            outcome.category(), outcome.message(), outcome.rule());
        return Medium(outcome.require(loaded, "the sources could not be loaded"));
    }

    /// Loads what a discovery already established, over the claim it
    /// already holds. **The discovery is consumed**, which is why it is
    /// taken by value.
    Medium load_discovery(Discovery discovery)
    {
        detail::Outcome outcome;
        RemanenceMedium* loaded = remanence_session_load_discovery(
            get(), discovery.release(), outcome.category(), outcome.message(), outcome.rule());
        return Medium(outcome.require(loaded, "the discovery could not be loaded"));
    }

    /// The same, with the session cache bound declared here — where the
    /// medium comes into existence (P27).
    Medium load_discovery(Discovery discovery, std::uint64_t cache_bytes)
    {
        detail::Outcome outcome;
        RemanenceMedium* loaded = remanence_session_load_discovery_with_cache(
            get(), discovery.release(), cache_bytes, outcome.category(), outcome.message(),
            outcome.rule());
        return Medium(outcome.require(loaded, "the discovery could not be loaded"));
    }

    /// The declared form, for a format that records several device types
    /// and leaves the choice to the caller.
    Medium load_discovery_as(Discovery discovery, const std::string& device_type)
    {
        detail::Outcome outcome;
        RemanenceMedium* loaded = remanence_session_load_discovery_as(
            get(), discovery.release(), device_type.c_str(), outcome.category(), outcome.message(),
            outcome.rule());
        return Medium(outcome.require(loaded, "the discovery could not be loaded as that device"));
    }

    Medium load_discovery_as(Discovery discovery, const std::string& device_type,
                             std::uint64_t cache_bytes)
    {
        detail::Outcome outcome;
        RemanenceMedium* loaded = remanence_session_load_discovery_as_with_cache(
            get(), discovery.release(), device_type.c_str(), cache_bytes, outcome.category(),
            outcome.message(), outcome.rule());
        return Medium(outcome.require(loaded, "the discovery could not be loaded as that device"));
    }

    std::size_t media_count() const noexcept { return remanence_session_media_count(get()); }

    /// The `index`th pooled medium's identity, in pool order.
    std::uint64_t media_id(std::size_t index) const
    {
        detail::in_range(index, media_count(), "media pool index");
        return remanence_session_media_id(get(), index);
    }

    std::vector<std::uint64_t> media() const
    {
        const std::size_t count = media_count();
        std::vector<std::uint64_t> ids;
        ids.reserve(count);
        for (std::size_t at = 0; at < count; at += 1) {
            ids.push_back(remanence_session_media_id(get(), at));
        }
        return ids;
    }

    /// One pooled medium, absent where the pool holds no such identity.
    std::optional<Medium> medium(std::uint64_t media_id)
    {
        RemanenceMedium* found = remanence_session_medium(get(), media_id);
        if (found == nullptr) {
            return std::nullopt;
        }
        return Medium(found);
    }

    /// Severs the medium's own link, then ends its claim. This is the
    /// one verb that destroys state.
    void release_media(std::uint64_t media_id)
    {
        detail::Outcome outcome;
        outcome.require(remanence_session_release_media(get(), media_id, outcome.category(),
                                                        outcome.message(), outcome.rule()),
                        "the medium could not be released");
    }

    // --- devices

    /// Adds a device in the lowest free slot of its bay.
    StorageDevice add_device(const std::string& slot)
    {
        detail::Outcome outcome;
        RemanenceDevice* added = remanence_session_add_device(
            get(), slot.c_str(), outcome.category(), outcome.message(), outcome.rule());
        return StorageDevice(outcome.require(added, "the device could not be added"));
    }

    StorageDevice add_device_at(const std::string& slot, std::uint32_t index)
    {
        detail::Outcome outcome;
        RemanenceDevice* added = remanence_session_add_device_at(
            get(), slot.c_str(), index, outcome.category(), outcome.message(), outcome.rule());
        return StorageDevice(outcome.require(added, "the device could not be added there"));
    }

    /// The one convenience over discovery: a device of the
    /// format-declared default family, with the artifact seated in it.
    StorageDevice add_device_for(const std::string& path,
                                 AccessIntent intent = AccessIntent::Read)
    {
        detail::Outcome outcome;
        RemanenceDevice* added = remanence_session_add_device_for(
            get(), path.c_str(), static_cast<RemanenceAccessIntent>(intent), outcome.category(),
            outcome.message(), outcome.rule());
        return StorageDevice(outcome.require(added, "no device was added for that artifact"));
    }

    /// Ejects first, then removes the device from the session.
    void release_device(const std::string& attachment)
    {
        detail::Outcome outcome;
        outcome.require(remanence_session_release_device(get(), attachment.c_str(),
                                                         outcome.category(), outcome.message(),
                                                         outcome.rule()),
                        "the device could not be released");
    }

    std::size_t device_count() const noexcept { return remanence_session_device_count(get()); }

    std::optional<std::string> device_attachment(std::size_t index) const
    {
        char* attachment = nullptr;
        if (!remanence_session_device_attachment(get(), index, &attachment)) {
            return std::nullopt;
        }
        return detail::owned_text(attachment);
    }

    std::vector<std::string> device_attachments() const
    {
        const std::size_t count = device_count();
        std::vector<std::string> found;
        found.reserve(count);
        for (std::size_t at = 0; at < count; at += 1) {
            std::optional<std::string> attachment = device_attachment(at);
            if (!attachment.has_value()) {
                break;
            }
            found.push_back(std::move(*attachment));
        }
        return found;
    }

    /// One device by attachment identity, absent where the session holds
    /// none.
    std::optional<StorageDevice> device(const std::string& attachment)
    {
        RemanenceDevice* found = remanence_session_device(get(), attachment.c_str());
        if (found == nullptr) {
            return std::nullopt;
        }
        return StorageDevice(found);
    }
};

} // namespace remanence

#endif // REMANENCE_HPP
