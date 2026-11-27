//! Hiding a folder on Windows (#6892). A design's work folder (`.~Counter.sc.parts`, where its
//! internal components and drawings are unpacked while it is open) is named with a leading dot,
//! which hides it on Linux and macOS and means nothing on Windows. Here the folder gets the
//! Hidden attribute, so it shows only to someone who has turned hidden items on, as the lock
//! file's name hides it elsewhere. The engine makes the folder and calls [`hide`] through
//! `solidcraft_engine::set_work_folder_hider`; it has no platform code of its own.
//!
//! Setting a file attribute has no safe API in the standard library, hence the `unsafe_code`
//! allowance on [`hide`].

use std::os::windows::ffi::OsStrExt as _;
use std::path::Path;

use windows_sys::Win32::Storage::FileSystem::{FILE_ATTRIBUTE_HIDDEN, GetFileAttributesW, INVALID_FILE_ATTRIBUTES, SetFileAttributesW};

/// Give the folder at `path` the Hidden attribute, keeping the attributes it has. A folder that
/// can't be read or changed (a read-only share) stays as it is: nothing depends on it being hidden.
#[allow(unsafe_code)]
pub fn hide(path: &Path) {
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    // A path with a NUL inside is not one Windows can name.
    if wide.iter().filter(|unit| **unit == 0).count() != 1 {
        return;
    }
    // SAFETY: `wide` is a NUL-terminated UTF-16 string that lives across both calls, and neither
    // call keeps the pointer.
    unsafe {
        let attributes = GetFileAttributesW(wide.as_ptr());
        if attributes != INVALID_FILE_ATTRIBUTES && attributes & FILE_ATTRIBUTE_HIDDEN == 0 {
            SetFileAttributesW(wide.as_ptr(), attributes | FILE_ATTRIBUTE_HIDDEN);
        }
    }
}
