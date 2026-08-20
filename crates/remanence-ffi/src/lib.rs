// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! C ABI for the Remanence disk image analysis library.
//!
//! Conventions:
//! - Handles (`RemanenceIdentification`, `RemanencePartition`,
//!   `RemanenceSpace`, `RemanenceFile`, `RemanenceDiskReport`) are
//!   opaque and freed with their matching `*_free` function.
//! - `const char*` return values are UTF-8, owned by the handle they were read
//!   from, and valid until that handle is freed. Do not free them.
//! - Fallible calls take optional category, message and rule outputs; on
//!   failure they store a stable [`RemanenceErrorCategory`](abi::RemanenceErrorCategory),
//!   a message to free
//!   with `remanence_string_free`, and — where the refusal came from an
//!   enumerated rule set — the stable identity of the rule that was broken,
//!   also freed with `remanence_string_free`. The rule output is null where
//!   no rule set applies, which is ordinary rather than an omission: the
//!   category says how to behave, and the rule says which rule the input
//!   broke. Rule sets belong to the seam that defines them and are documented
//!   there — the DOS 8.3 namespace's is the set the file verbs draw on — so
//!   the identity is a string rather than a second library-wide enum.
//! - Accessors taking an index return null / false / 0 when the index is out of
//!   range or the field does not apply to the layer's layout.
//!
//! The modules below group the `remanence_*` functions by the prefix they
//! carry, so the module a function lives in is a lookup rather than a
//! judgement: `remanence_partition_*` is [`storage::partition`],
//! `remanence_bytestream_*` is [`flux::stream`], and so on. Nothing the
//! exported ABI promises depends on that grouping — the symbols are
//! `#[unsafe(no_mangle)]` and carry no module path, so a function may be
//! moved between modules freely.
//!
//! What a move *does* reach is the order of `c/include/remanence.h`, which
//! cbindgen emits in module-declaration order under `[fn] sort_by = "None"`.
//! rustfmt keeps these declarations alphabetical, so the header groups by
//! module and orders those groups by name; moving a function to another
//! module reorders the header without changing a line of it. Regenerate and
//! commit the header in the same change.

pub mod abi;
pub mod assurance;
pub mod catalog;
pub mod device;
pub mod discovery;
pub mod flux;
pub mod geometry;
pub mod identify;
pub mod medium;
pub mod report;
pub mod session;
pub mod storage;

use std::ffi::{CString, c_char};

/// Counting live allocations, so a C caller can prove the `_free`
/// discipline (D47).
///
/// **Everything this ABI hands out is allocated by Rust inside this
/// cdylib** — `CString::into_raw` for strings, `Box::into_raw` for
/// handles — and freed by Rust when the matching `remanence_*_free`
/// runs. A C-side leak checker cannot see any of it: CppUTest, and the
/// sanitizers, instrument the *test binary's* allocator, which these
/// allocations never touch. So the count has to come from in here.
///
/// Off by default and absent from a released artifact: it is a global
/// allocator and an exported symbol, and an extra `remanence_*` symbol
/// would be an S2 change.
#[cfg(feature = "leak-probe")]
mod leak_probe {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::sync::atomic::{AtomicI64, Ordering};

    static LIVE: AtomicI64 = AtomicI64::new(0);

    /// Counts blocks rather than bytes: the question is whether every
    /// allocation was given back, and a block is what a `_free` returns.
    pub struct Counting;

    // SAFETY: every method forwards to `System` unchanged and only adds
    // an atomic to the bookkeeping, so the allocator contract is
    // whatever `System` already satisfies.
    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            let pointer = unsafe { System.alloc(layout) };
            if !pointer.is_null() {
                LIVE.fetch_add(1, Ordering::Relaxed);
            }
            pointer
        }

        unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
            let pointer = unsafe { System.alloc_zeroed(layout) };
            if !pointer.is_null() {
                LIVE.fetch_add(1, Ordering::Relaxed);
            }
            pointer
        }

        unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
            LIVE.fetch_sub(1, Ordering::Relaxed);
            unsafe { System.dealloc(pointer, layout) }
        }

        // `realloc` is deliberately left to the trait's default, which
        // allocates, copies and deallocates through the methods above —
        // so a growing buffer nets to zero rather than needing its own
        // rule.
    }

    pub fn live() -> i64 {
        LIVE.load(Ordering::Relaxed)
    }
}

#[cfg(feature = "leak-probe")]
#[global_allocator]
pub(crate) static LEAK_PROBE: leak_probe::Counting = leak_probe::Counting;

/// How many Rust allocations inside this library are live right now.
///
/// Test-only, and present only under the `leak-probe` feature — it is
/// deliberately **not** in the generated header, because it is not part
/// of S2. A caller declares it itself.
#[cfg(feature = "leak-probe")]
#[unsafe(no_mangle)]
pub extern "C" fn remanence_probe_live_allocations() -> i64 {
    leak_probe::live()
}

/// Returns the library version as a static string. Do not free.
#[unsafe(no_mangle)]
pub extern "C" fn remanence_version() -> *const c_char {
    concat!(env!("CARGO_PKG_VERSION"), "\0").as_ptr().cast()
}

/// The stated default session cache bound, in bytes: what an open
/// without a declared bound uses.
#[unsafe(no_mangle)]
pub extern "C" fn remanence_default_cache_bytes() -> u64 {
    remanence::DEFAULT_CACHE_BYTES
}

/// Frees a string returned through an `error_out` or `error_rule_out`
/// parameter.
///
/// A fallible call writes three things on failure: the stable
/// category, which says how to behave; the human diagnostic; and,
/// where the refusal is one of an enumerated set of rules a format,
/// namespace, or grammar defines, the stable identity of the rule the input
/// broke. `error_rule_out` is null where no such rule set applies, which is
/// the ordinary case rather than an omission — the rule identity never
/// substitutes for the category. Each output is optional; passing null for
/// any of them declines it. The DOS 8.3 namespace owns the set the file
/// verbs draw on: `empty-base`, `base-too-long`, `extension-too-long`,
/// `separator`, `excluded-character`, `reserved-device-name`,
/// `surrounding-space`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn remanence_string_free(string: *mut c_char) {
    if !string.is_null() {
        drop(unsafe { CString::from_raw(string) });
    }
}

/// One journey through the whole surface, which is why it sits at the root
/// rather than in any single module: it loads a degraded medium, reads what
/// the assurance says of it, and watches a write be withheld for the reason
/// the assurance gave.
#[cfg(test)]
mod tests {
    use super::remanence_string_free;
    use crate::abi::*;
    use crate::assurance::*;
    use crate::catalog::*;
    use crate::medium::*;
    use crate::session::*;

    use remanence::Format;
    use std::ffi::CStr;
    use std::ptr;

    /// The raw OS handle of a file the test hands to the library, which
    /// takes ownership of it from there.
    fn raw_source(file: std::fs::File) -> isize {
        #[cfg(windows)]
        {
            use std::os::windows::io::IntoRawHandle;
            file.into_raw_handle() as isize
        }
        #[cfg(not(windows))]
        {
            use std::os::fd::IntoRawFd;
            file.into_raw_fd() as isize
        }
    }

    /// A 1.44 MiB FAT12 floppy holding one file whose cluster chain runs
    /// past `keep` bytes, then truncated to `keep` — the shape P28's
    /// degraded reading is stated over.
    fn truncated_floppy(path: &std::path::Path, keep: u64) {
        const TOTAL_SECTORS: usize = 2880;
        let mut image = vec![0u8; TOTAL_SECTORS * 512];
        image[0] = 0xeb;
        image[1] = 0x3c;
        image[2] = 0x90;
        image[3..11].copy_from_slice(b"REMANENC");
        image[11..13].copy_from_slice(&512u16.to_le_bytes());
        image[13] = 1;
        image[14..16].copy_from_slice(&1u16.to_le_bytes());
        image[16] = 2;
        image[17..19].copy_from_slice(&224u16.to_le_bytes());
        image[19..21].copy_from_slice(&(TOTAL_SECTORS as u16).to_le_bytes());
        image[21] = 0xf0;
        image[22..24].copy_from_slice(&9u16.to_le_bytes());
        image[24..26].copy_from_slice(&18u16.to_le_bytes());
        image[26..28].copy_from_slice(&2u16.to_le_bytes());
        image[510] = 0x55;
        image[511] = 0xaa;
        for fat in 0..2usize {
            let base = (1 + fat * 9) * 512;
            image[base] = 0xf0;
            image[base + 1] = 0xff;
            image[base + 2] = 0xff;
        }
        std::fs::write(path, image).expect("image writes");

        // The file is written through the library's own writer, so the
        // chain the truncation cuts is a real one.
        let mut session = remanence::Session::new();
        let source = std::fs::File::options()
            .read(true)
            .write(true)
            .open(path)
            .expect("the caller's own writable open");
        let medium = session
            .load_media(
                source,
                Format::Raw {
                    device: remanence::HardDrive::MbrSector.into(),
                    block_bytes: 512,
                },
            )
            .expect("the whole image loads");
        let content: Vec<u8> = (0..1_200_000u32).map(|n| (n % 241) as u8).collect();
        medium
            .partition(0)
            .expect("a partitionless floppy bears its direct partition")
            .filesystem_as("fat")
            .expect("the declared reading the boot record bears out")
            .write_file("FAR.BIN", &content)
            .expect("writes");
        medium.commit().expect("commits");
        drop(session);

        std::fs::OpenOptions::new()
            .write(true)
            .open(path)
            .expect("opens for truncation")
            .set_len(keep)
            .expect("truncates");
    }

    /// The C presentation of P28 carries what Rust's does: the outcome,
    /// the condition, the ordered evidence, the exact readable extent,
    /// and the effective access mode — and a withheld write names the
    /// same condition as its rule (P5).
    #[test]
    fn the_c_surface_reports_a_degraded_medium_and_withholds_its_writes() {
        let path =
            std::env::temp_dir().join(format!("remanence-ffi-degraded-{}.img", std::process::id()));
        truncated_floppy(&path, 1_000_000);

        let session = unsafe { remanence_session_new() };
        let format = to_cstring("raw");
        let device = to_cstring("mbr-sector-hd");
        let mut category = RemanenceErrorCategory::Io;
        let mut message = ptr::null_mut();
        let mut rule = ptr::null_mut();
        // The caller's own open, handed over: the library takes the
        // handle and asks it one question (P7 as amended).
        let source = std::fs::File::options()
            .read(true)
            .write(true)
            .open(&path)
            .expect("the caller's own writable open");
        let medium = unsafe {
            remanence_session_load_media(
                session,
                raw_source(source),
                format.as_ptr(),
                device.as_ptr(),
                512,
                &mut category,
                &mut message,
                &mut rule,
            )
        };
        assert!(
            !medium.is_null(),
            "a truncated source still loads, degraded"
        );

        let assurance = unsafe { remanence_medium_assurance(medium) };
        assert_eq!(
            unsafe { remanence_assurance_outcome(assurance) },
            RemanenceAssuranceOutcome::Degraded
        );
        assert_eq!(
            unsafe { CStr::from_ptr(remanence_assurance_condition(assurance)) }
                .to_str()
                .expect("UTF-8"),
            "source-truncated"
        );
        assert_eq!(
            unsafe { remanence_assurance_access_mode(assurance) },
            RemanenceAccessMode::ReadOnly
        );
        assert_eq!(
            unsafe { remanence_medium_mode(medium) },
            RemanenceAccessMode::ReadOnly,
            "the effective mode is the same answer read another way"
        );
        assert_eq!(
            unsafe { remanence_assurance_claim(assurance) },
            RemanenceClaim::CallerOpened,
            "the claim's class travels beside the access it established"
        );
        assert!(unsafe { remanence_assurance_evidence_count(assurance) } > 0);
        assert!(
            !unsafe { remanence_assurance_evidence(assurance, 0) }.is_null(),
            "the declaration leads the evidence"
        );
        assert!(unsafe { remanence_assurance_evidence(assurance, 99) }.is_null());

        let mut declared = 0u64;
        let mut observed = 0u64;
        let mut first_unavailable = 0u64;
        assert!(unsafe { remanence_assurance_declared_bytes(assurance, &mut declared) });
        assert!(unsafe { remanence_assurance_observed_bytes(assurance, &mut observed) });
        assert!(unsafe {
            remanence_assurance_first_unavailable_byte(assurance, &mut first_unavailable)
        });
        assert_eq!(declared, 1_474_560);
        assert_eq!(observed, 1_000_000);
        assert_eq!(first_unavailable, 1_000_000);

        assert_eq!(unsafe { remanence_assurance_readable_count(assurance) }, 1);
        let mut start = u64::MAX;
        let mut end = 0u64;
        assert!(unsafe { remanence_assurance_readable(assurance, 0, &mut start, &mut end) });
        assert_eq!((start, end), (0, 1_000_000));
        assert!(!unsafe { remanence_assurance_readable(assurance, 1, &mut start, &mut end) });
        unsafe { remanence_assurance_free(assurance) };

        // Every mutation path carries the condition as its rule.
        assert!(
            !unsafe { remanence_medium_commit(medium, &mut category, &mut message, &mut rule) },
            "commit is denied"
        );
        assert_eq!(category, RemanenceErrorCategory::ReadOnly);
        assert_eq!(
            unsafe { CStr::from_ptr(rule) }.to_str().expect("UTF-8"),
            "source-truncated"
        );
        unsafe { remanence_string_free(message) };
        unsafe { remanence_string_free(rule) };

        // The claimed condition set is readable without meeting one.
        assert_eq!(remanence_assurance_condition_count(), 2);
        assert_eq!(
            unsafe { CStr::from_ptr(remanence_assurance_condition_name(1)) }
                .to_str()
                .expect("UTF-8"),
            "evidence-conflict"
        );
        assert!(remanence_assurance_condition_name(2).is_null());

        unsafe { remanence_session_free(session) };
        std::fs::remove_file(&path).ok();
    }
}
