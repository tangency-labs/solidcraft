//! The codes of the features that failed lately, by their error's text: what a report of a
//! failure the window shows (a dialog's error, a preview's, a message) finds its kernel call's
//! stable code by, wherever the error was turned into words on its way there.
//!
//! Filled where a failed feature becomes a command's error ([`crate::EngineError::Feature`]),
//! on whatever thread that ran (a preview's worker included); the newest few are kept.

use std::collections::VecDeque;
use std::sync::{Mutex, PoisonError};

use solidcraft_kernel::ErrorCode;

/// How many recent failures are kept.
const KEPT: usize = 32;

static RECENT: Mutex<VecDeque<(String, ErrorCode)>> = Mutex::new(VecDeque::new());

/// Notes that a feature failed with `error`, made from the kernel call with `code`.
pub(crate) fn note(error: &str, code: &ErrorCode) {
    if error.is_empty() {
        return;
    }
    let mut recent = RECENT.lock().unwrap_or_else(PoisonError::into_inner);
    recent.retain(|(e, _)| e != error);
    if recent.len() >= KEPT {
        recent.pop_front();
    }
    recent.push_back((error.to_owned(), code.clone()));
}

/// The stable code of the kernel call a recent feature failure was made from, for `error` as the
/// window shows it: the newest failure whose error `error` carries, or that `error` is the start
/// of (a message cut short). `None` for an error that was made from no kernel call's.
pub fn failure_code_of(error: &str) -> Option<ErrorCode> {
    let error = error.trim();
    if error.is_empty() {
        return None;
    }
    let recent = RECENT.lock().unwrap_or_else(PoisonError::into_inner);
    recent.iter().rev().find(|(e, _)| error.contains(e.as_str()) || e.starts_with(error)).map(|(_, c)| c.clone())
}
