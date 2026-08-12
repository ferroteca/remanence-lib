// SPDX-FileCopyrightText: 2026 Paul Galbraith
// SPDX-License-Identifier: GPL-3.0-only

//! The caller-owned claim: what a handed-over file handle affords, and
//! the name recovered from it.
//!
//! **Whoever opens owns the lock.** A local artifact reaches
//! [`Session::load_media`](crate::Session::load_media) as the caller's
//! own opened [`std::fs::File`], and that open *is* the P7 claim: it is
//! the caller's safeguard and the library's claim at once. In-force P7's
//! mandatory write-denial is scoped accordingly — mandatory where the
//! library opens, caller-owned where the caller opened — so nothing here
//! takes a lock of its own, and nothing escalates one.
//!
//! The library checks the handle for **exactly one thing**: may it write
//! through it? The answer is honoured exactly — a read-only handle makes
//! a read-only medium whose write verbs refuse by name, and the class of
//! the claim travels on the medium's assurance
//! ([`Claim`](crate::device::Claim)).
//!
//! **A name recovered from a handle serves location only.** Two journeys
//! need to know where an artifact sits rather than what it holds: the
//! commit journal lands *beside* the file (P9), and a backing chain's
//! parent is searched for *next door* (U6, D18). Both are recovered from
//! the handle itself and checked to still denote the handle's own file,
//! so a name is never trusted as a second way in. A **nameless handle** —
//! memory-only, deleted-but-open, or one the host will not name — refuses
//! those journeys by name and serves everything else.

use std::fs::File;
use std::path::PathBuf;

use crate::device::AccessMode;

/// What the caller's handle affords this session — the one question P7
/// asks of a claim it did not take.
///
/// It is asked by attempting a **zero-length write**: the kernel checks
/// the handle's access before it looks at the length, so the answer is
/// exact and not one byte changes — no content, no length, no
/// modification time.
pub(crate) fn afforded_access(file: &File) -> AccessMode {
    if writes_through(file) {
        AccessMode::ReadWrite
    } else {
        AccessMode::ReadOnly
    }
}

#[cfg(windows)]
fn writes_through(file: &File) -> bool {
    use std::os::windows::fs::FileExt;
    file.seek_write(&[], 0).is_ok()
}

#[cfg(not(windows))]
fn writes_through(file: &File) -> bool {
    use std::os::unix::fs::FileExt;
    file.write_at(&[], 0).is_ok()
}

/// The name `file` was opened by, recovered from the handle for
/// **location only**, or `None` where there is no such name to be had.
///
/// The recovery is from the handle rather than from anything the caller
/// said, and the identity check is what makes it usable: the recovered
/// name is only answered when it still denotes the very file the handle
/// holds, so a rename or a replacement between the open and this call
/// produces an absence rather than a wrong neighbourhood.
pub(crate) fn recovered_name(file: &File) -> Option<PathBuf> {
    let name = platform::name_of(file)?;
    let by_name = platform::identity_of_path(&name)?;
    let by_handle = platform::identity_of(file)?;
    (by_name == by_handle).then_some(name)
}

#[cfg(windows)]
mod platform {
    use std::ffi::c_void;
    use std::fs::{File, OpenOptions};
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    use std::path::{Path, PathBuf};

    /// `BY_HANDLE_FILE_INFORMATION`, whose layout the Win32 API fixes.
    #[repr(C)]
    #[derive(Default)]
    struct ByHandleFileInformation {
        file_attributes: u32,
        creation_time: [u32; 2],
        last_access_time: [u32; 2],
        last_write_time: [u32; 2],
        volume_serial_number: u32,
        file_size_high: u32,
        file_size_low: u32,
        number_of_links: u32,
        file_index_high: u32,
        file_index_low: u32,
    }

    unsafe extern "system" {
        fn GetFinalPathNameByHandleW(
            file: *mut c_void,
            path: *mut u16,
            count: u32,
            flags: u32,
        ) -> u32;
        fn GetFileInformationByHandle(
            file: *mut c_void,
            information: *mut ByHandleFileInformation,
        ) -> i32;
    }

    /// The file this handle holds, as the volume and index that identify
    /// it — the pair Windows itself uses to say "the same file".
    pub(super) type Identity = (u32, u32, u32);

    pub(super) fn name_of(file: &File) -> Option<PathBuf> {
        // 32 KiB of UTF-16 covers the extended-length path maximum, so a
        // second call to size the buffer buys nothing.
        let mut buffer = vec![0u16; 32 * 1024];
        let written = unsafe {
            GetFinalPathNameByHandleW(
                file.as_raw_handle().cast(),
                buffer.as_mut_ptr(),
                buffer.len() as u32,
                0,
            )
        };
        if written == 0 || written as usize >= buffer.len() {
            return None;
        }
        Some(PathBuf::from(shorten(&String::from_utf16_lossy(
            &buffer[..written as usize],
        ))))
    }

    /// Trims the extended-length prefix the final-path form always
    /// carries, so a recovered name reads the way the caller's own does.
    /// A UNC path keeps its own shape.
    fn shorten(path: &str) -> String {
        if let Some(rest) = path.strip_prefix(r"\\?\UNC\") {
            return format!(r"\\{rest}");
        }
        match path.strip_prefix(r"\\?\") {
            // Only a plain drive-letter path is shortened: a volume-GUID
            // name needs its prefix to mean anything at all.
            Some(rest) if is_drive_path(rest) => rest.to_owned(),
            _ => path.to_owned(),
        }
    }

    fn is_drive_path(path: &str) -> bool {
        let mut characters = path.chars();
        matches!(
            (characters.next(), characters.next(), characters.next()),
            (Some(letter), Some(':'), Some('\\')) if letter.is_ascii_alphabetic()
        )
    }

    pub(super) fn identity_of(file: &File) -> Option<Identity> {
        let mut information = ByHandleFileInformation::default();
        let ok = unsafe {
            GetFileInformationByHandle(file.as_raw_handle().cast(), &raw mut information)
        };
        (ok != 0).then_some((
            information.volume_serial_number,
            information.file_index_high,
            information.file_index_low,
        ))
    }

    pub(super) fn identity_of_path(path: &Path) -> Option<Identity> {
        // Desired access zero asks for the file's metadata and nothing
        // else, and every sharing mode is admitted, so the check never
        // contends with the caller's own claim — or with anyone else's.
        const FILE_SHARE_ALL: u32 = 0x1 | 0x2 | 0x4;
        const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
        let opened = OpenOptions::new()
            .access_mode(0)
            .share_mode(FILE_SHARE_ALL)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
            .open(path)
            .ok()?;
        identity_of(&opened)
    }
}

#[cfg(not(windows))]
mod platform {
    use std::fs::File;
    use std::os::unix::fs::MetadataExt;
    use std::path::{Path, PathBuf};

    /// The device and inode that identify a file to POSIX.
    pub(super) type Identity = (u64, u64);

    #[cfg(target_os = "linux")]
    pub(super) fn name_of(file: &File) -> Option<PathBuf> {
        use std::os::fd::AsRawFd;
        let link = std::fs::read_link(format!("/proc/self/fd/{}", file.as_raw_fd())).ok()?;
        // An unlinked file's link is decorated rather than absent, and a
        // decorated name denotes nothing: that is a nameless handle.
        (link.is_absolute() && !link.to_string_lossy().ends_with(" (deleted)")).then_some(link)
    }

    #[cfg(target_os = "macos")]
    pub(super) fn name_of(file: &File) -> Option<PathBuf> {
        use std::os::fd::AsRawFd;
        const F_GETPATH: i32 = 50;
        const PATH_MAX: usize = 1024;
        unsafe extern "C" {
            fn fcntl(fd: i32, command: i32, ...) -> i32;
        }
        let mut buffer = [0u8; PATH_MAX];
        if unsafe { fcntl(file.as_raw_fd(), F_GETPATH, buffer.as_mut_ptr()) } != 0 {
            return None;
        }
        let end = buffer.iter().position(|byte| *byte == 0)?;
        let name = PathBuf::from(String::from_utf8_lossy(&buffer[..end]).into_owned());
        name.is_absolute().then_some(name)
    }

    /// A host this release cannot ask names names nothing, which is the
    /// nameless-handle answer rather than a guess.
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    pub(super) fn name_of(_file: &File) -> Option<PathBuf> {
        None
    }

    pub(super) fn identity_of(file: &File) -> Option<Identity> {
        let metadata = file.metadata().ok()?;
        Some((metadata.dev(), metadata.ino()))
    }

    pub(super) fn identity_of_path(path: &Path) -> Option<Identity> {
        let metadata = std::fs::metadata(path).ok()?;
        Some((metadata.dev(), metadata.ino()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "remanence-handle-{tag}-{}.bin",
            std::process::id()
        ))
    }

    #[test]
    fn a_handle_is_asked_only_whether_it_may_write() {
        let path = scratch("afforded");
        std::fs::write(&path, vec![0x5Au8; 4096]).expect("scratch writes");
        let before = std::fs::metadata(&path).expect("stat");

        let reading = File::open(&path).expect("opens for reading");
        assert_eq!(afforded_access(&reading), AccessMode::ReadOnly);
        let writing = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .expect("opens for writing");
        assert_eq!(afforded_access(&writing), AccessMode::ReadWrite);

        // The question costs the artifact nothing: asking is not writing.
        let after = std::fs::metadata(&path).expect("stat");
        assert_eq!(after.len(), before.len());
        assert_eq!(
            std::fs::read(&path).expect("reads back"),
            vec![0x5Au8; 4096]
        );
        drop((reading, writing));
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn a_name_is_recovered_from_the_handle_and_denotes_it() {
        let path = scratch("named");
        std::fs::write(&path, b"located").expect("scratch writes");
        let file = File::open(&path).expect("opens");
        let recovered = recovered_name(&file).expect("this host names its handles");
        assert_eq!(
            std::fs::canonicalize(&recovered).expect("recovered name resolves"),
            std::fs::canonicalize(&path).expect("original resolves"),
            "the recovered name denotes the file the handle holds"
        );
        drop(file);
        std::fs::remove_file(&path).ok();
    }
}
