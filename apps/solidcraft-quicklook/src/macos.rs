//! What both extensions share: handing the process to the extension host, and their replies'
//! error.
//!
//! An app extension's process is run by Foundation's `NSExtensionMain`, which reads the bundle's
//! `NSExtension` dictionary, looks up its principal class by name and answers the host's requests
//! with it. That function, and the Objective-C methods each extension implements, have no safe API,
//! hence the `unsafe_code` allowance on this module and on each extension's `provider` module.
#![allow(unsafe_code)]

use std::ffi::{CString, c_char, c_int};
use std::path::PathBuf;

use objc2::rc::Retained;
use objc2_foundation::{NSError, NSString, NSURL};

unsafe extern "C" {
    /// Foundation's entry point for app extensions; it doesn't return while the host needs us.
    fn NSExtensionMain(argc: c_int, argv: *mut *mut c_char) -> c_int;
}

/// Run the extension whose principal class `register` has registered with the runtime.
pub fn run(register: fn()) -> i32 {
    register();
    // The host passes no arguments that matter, but NSExtensionMain takes the usual argc/argv.
    let args: Vec<CString> = std::env::args_os().filter_map(|a| CString::new(a.into_encoded_bytes()).ok()).collect();
    let mut argv: Vec<*mut c_char> = args.iter().map(|a| a.as_ptr().cast_mut()).collect();
    argv.push(std::ptr::null_mut());
    let argc = c_int::try_from(args.len()).unwrap_or(0);
    // SAFETY: argv is a null-terminated array of `argc` NUL-terminated strings that outlive the
    // call (`args` and `argv` live until this function returns, after NSExtensionMain).
    unsafe { NSExtensionMain(argc, argv.as_mut_ptr()) }
}

/// The file a request names, if it is a local file.
pub fn file(url: &NSURL) -> Option<PathBuf> {
    if url.isFileURL() { url.to_file_path() } else { None }
}

/// The error a reply carries when a design has nothing to show: the host then shows the
/// generic icon or card.
pub fn nothing_to_show() -> Retained<NSError> {
    // NSFileReadCorruptFileError: the file isn't a design this reader can show.
    NSError::new(259, &NSString::from_str("NSCocoaErrorDomain"))
}
