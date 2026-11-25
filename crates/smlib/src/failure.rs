//! What the last kernel call that failed on this thread failed with, as stable data: its
//! [`FailureCode`] (the call, the kernel's kind of failure, the kernel routine), beside the
//! message of the error this crate gave for it. An application keeps the code with the error
//! the failure became (a feature's result, a report) to group and replay failures by, without
//! reading their words.
//!
//! Per thread: a call fails on the thread that made it, and calls on other threads at the same
//! time leave theirs on their own. Every call this crate makes on the kernel, through
//! solidcore-ops or on a kernel of its own, sets it when it fails (a cancelled call is no
//! failure and sets none); nothing clears it but [`clear`] and the next failure.

use std::cell::RefCell;

pub use solidcore_ops::FailureCode;

thread_local! {
    static LAST: RefCell<Option<(FailureCode, String)>> = const { RefCell::new(None) };
}

/// Notes that the call failed with `code`, as this crate's error `e`.
pub(crate) fn failed(code: FailureCode, e: &crate::Error) {
    let message = e.to_string();
    // (a thread being torn down notes nothing)
    let _ = LAST.try_with(|l| l.replace(Some((code, message))));
}

/// Forgets the last failure on this thread: an application clears it before the work whose
/// failure it will ask about.
pub fn clear() {
    let _ = LAST.try_with(|l| l.replace(None));
}

/// The last failure on this thread: its code and the message of the error this crate gave.
#[must_use]
pub fn last() -> Option<(FailureCode, String)> {
    LAST.try_with(|l| l.borrow().clone()).ok().flatten()
}

/// The code of the last failure on this thread when `error`, an error's text as the layers
/// above word it, carries that failure's message: the failure `error` was made from. `None`
/// when there was none, or when `error` says something else (a failure of the application's
/// own, after a kernel call that failed and was recovered from).
#[must_use]
pub fn code_in(error: &str) -> Option<FailureCode> {
    let (code, message) = last()?;
    (!message.is_empty() && error.contains(&message)).then_some(code)
}

#[cfg(test)]
#[path = "failure_tests.rs"]
mod tests;
