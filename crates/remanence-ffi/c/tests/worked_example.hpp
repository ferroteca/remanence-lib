/*
 * SPDX-FileCopyrightText: 2026 Paul Galbraith
 * SPDX-License-Identifier: GPL-3.0-only
 */

/*
 * The remanence format's own worked example, built rather than fetched.
 *
 * Twenty-one bytes of payload — one index hole at 3/8 of a turn, one
 * surface, one orbit at 57,150 µm holding two points — wrapped in the
 * artifact's magic, sentinel and version, with the payload as a single
 * stored DEFLATE block inside a zlib stream. That is the whole grammar,
 * and emitting it needs no compressor, so any C++ caller can lay a valid
 * `.remanence` artifact on disk and open it.
 *
 * **Two points are not a recording, and that is the point.** The image,
 * its shape, its round trip and the first rungs of the ladder all
 * answer; the sector layer refuses by name because nothing frames a
 * record. Both are surface. A real recording is the fixture-gated
 * `flux_capture` group's business.
 *
 * Shared by `wrapper.cpp`, which checks what the ladder answers, and
 * `wrapper_leaks.cpp`, which checks that its handles give back what they
 * took.
 */

#ifndef REMANENCE_WORKED_EXAMPLE_HPP
#define REMANENCE_WORKED_EXAMPLE_HPP

#include <cstdint>
#include <cstdio>
#include <fstream>
#include <ios>
#include <string>
#include <vector>

namespace worked_example {

/// Form factor, one hole, one surface, one orbit, two points.
inline const std::uint8_t PAYLOAD[] = {
    0x01,             // form factor: 5.25-inch
    0x01,             // one hole
    0x06, 0x08,       // 3/8 of a turn
    0x02, 0x32,       // 1/50 extent
    0x01,             // one surface
    0x00,             // surface 0
    0x01,             // one orbit
    0xbe, 0xbe, 0x03, // centre radius 57150
    0x02,             // two points
    0x03, 0x00, 0xca, 0x02, 0xb0, 0x03, // +0, positive, plateau 330, guard 432
    0xd0, 0x0f,                         // +500, alternating to negative
};

inline std::uint32_t adler32(const std::uint8_t* bytes, std::size_t length)
{
    std::uint32_t low = 1;
    std::uint32_t high = 0;
    for (std::size_t at = 0; at < length; at += 1) {
        low = (low + bytes[at]) % 65521;
        high = (high + low) % 65521;
    }
    return (high << 16) | low;
}

/// The whole artifact: header, then the payload as one stored DEFLATE
/// block in a zlib stream — the simplest valid encoding, and one the
/// library's own inflater reads like any other.
inline std::vector<std::uint8_t> artifact()
{
    const std::size_t length = sizeof(PAYLOAD);
    std::vector<std::uint8_t> out;
    const char* magic = "REMANENCE_PHYSICAL_DISK";
    while (*magic != '\0') {
        out.push_back(static_cast<std::uint8_t>(*magic));
        magic += 1;
    }
    out.push_back(0x1a); // sentinel
    out.push_back(0x01); // layout version

    out.push_back(0x78); // zlib header
    out.push_back(0x01);
    out.push_back(0x01); // one stored block, and it is the last
    out.push_back(static_cast<std::uint8_t>(length & 0xff));
    out.push_back(static_cast<std::uint8_t>((length >> 8) & 0xff));
    out.push_back(static_cast<std::uint8_t>(~length & 0xff));
    out.push_back(static_cast<std::uint8_t>((~length >> 8) & 0xff));
    out.insert(out.end(), PAYLOAD, PAYLOAD + length);

    const std::uint32_t sum = adler32(PAYLOAD, length);
    out.push_back(static_cast<std::uint8_t>(sum >> 24));
    out.push_back(static_cast<std::uint8_t>(sum >> 16));
    out.push_back(static_cast<std::uint8_t>(sum >> 8));
    out.push_back(static_cast<std::uint8_t>(sum));
    return out;
}

/// Lays the artifact at `path`, replacing whatever was there. Answers
/// false where the host would not have it, which the caller reports in
/// its own terms.
inline bool place(const std::string& path)
{
    const std::vector<std::uint8_t> bytes = artifact();
    std::remove(path.c_str());
    // `std::ofstream` rather than `std::fopen`, which MSVC deprecates at
    // /W4 — and warnings are errors here.
    std::ofstream file(path, std::ios::binary | std::ios::trunc);
    if (!file) {
        return false;
    }
    file.write(reinterpret_cast<const char*>(bytes.data()),
               static_cast<std::streamsize>(bytes.size()));
    return static_cast<bool>(file);
}

} // namespace worked_example

#endif // REMANENCE_WORKED_EXAMPLE_HPP
