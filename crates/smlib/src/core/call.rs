//! How this crate makes a solidcore-ops call ([`ops`]): what [`super::run_on`] gives a call
//! this crate runs on the kernel itself, given to one solidcore-ops runs on its own session.
//!
//! - **Fixes:** the session's kernels run with this crate's fixes ([`super::fixes`]), named to
//!   it before the first call.
//! - **Stopping:** the token over this thread's work ([`crate::stop`]) is over the call.
//! - **Failures:** while a dump folder is set ([`super::dump::set_dir`]), a failed call's
//!   inputs are left there ([`super::dump::failed_ops`]), written only once it has failed.
//! - **The call running now** ([`super::dump::current`]) and the profiler's span, as
//!   [`super::run_on`] keeps them.
//! - **Errors:** solidcore-ops' error as this crate's ([`error`]).

use std::sync::Once;

use crate::{Error, Result};

/// Names this crate's fixes to solidcore-ops' session, once: every kernel it makes or hands out
/// from then on runs with them.
fn configured() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let fixes = super::fixes();
        let on = fixes.flags().filter(|(_, on)| *on).map(|(name, _)| name);
        if let Err(e) = solidcore_ops::session::set_fixes(on) {
            log::warn!(target: "smlib", "solidcore-ops' session did not take the fixes: {e}");
        }
    });
}

/// Runs the solidcore-ops call `f`, named `call` for the profiler, the crash reports and a
/// stopped call's error ([`ops_on`] with no inputs).
pub(crate) fn ops<T>(call: &'static str, f: impl FnOnce() -> solidcore_ops::Result<T>) -> Result<T> {
    ops_on(call, &[], f)
}

/// [`ops`] for a call that reads the bodies `inputs`: should it fail while a dump folder is set,
/// they are left there. They are the caller's bodies, not the copies a consuming call is handed.
pub(crate) fn ops_on<T>(call: &'static str, inputs: super::dump::Inputs<'_>, f: impl FnOnce() -> solidcore_ops::Result<T>) -> Result<T> {
    ops_as(call, inputs, error, f)
}

/// [`ops_on`] for a family whose refusals this crate has kinds of its own for: `kind` gives
/// solidcore-ops' error as this crate's (the family's kinds, then [`error`] for the rest).
pub(crate) fn ops_as<T>(
    call: &'static str,
    inputs: super::dump::Inputs<'_>,
    kind: fn(&'static str, solidcore_ops::Error, Option<String>) -> Error,
    f: impl FnOnce() -> solidcore_ops::Result<T>,
) -> Result<T> {
    configured();
    let stop = crate::stop::token();
    if stop.as_ref().is_some_and(solidcore_ops::cancel::CancelToken::is_cancelled) {
        return Err(Error::Cancelled(call));
    }
    let _span = solidcraft_profile::span("solidcore", call);
    let in_flight = super::dump::in_flight(call);
    // an older failure's words and code are not this call's
    drop(solidcore_ops::session::take_failure_detail());
    let _ = solidcore_ops::session::take_failure_code();
    let r = match &stop {
        Some(token) => solidcore_ops::cancel::within(token, f),
        None => f(),
    };
    drop(in_flight);
    r.map_err(|ops| {
        // the session's code for the call that failed; for an error solidcore-ops gave before
        // any kernel call, ops' own code of it on this crate's call
        let code = solidcore_ops::session::take_failure_code().unwrap_or_else(|| solidcore_ops::FailureCode::of_ops(call, &ops));
        let e = kind(call, ops, solidcore_ops::session::take_failure_detail());
        // Stopped, it is no failure to look into.
        if !matches!(e, Error::Cancelled(_)) {
            crate::failure::failed(code, &e);
            super::dump::failed_ops(call, &e.to_string(), inputs);
        }
        e
    })
}

/// solidcore-ops' error as this crate's: the ones this crate has a kind for as that kind, any
/// other as [`Error::Solidcore`] with the call's name before it. Where the call failed with a
/// kernel error, the text is the kernel's own statement of it (`detail`,
/// [`solidcore_ops::session::take_failure_detail`]), exactly as a kernel error of a call this
/// crate runs itself reads; else it is solidcore-ops' statement of its error.
pub(crate) fn error(call: &'static str, e: solidcore_ops::Error, detail: Option<String>) -> Error {
    use solidcore_ops::Error as Ops;
    // A body whose solid test is named is the same refusal to this crate's callers. The
    // variant is `#[non_exhaustive]`, so its field is read through the method.
    if e.not_solid_test().is_some() {
        return Error::NotSolid;
    }
    match e {
        Ops::Argument => Error::Argument,
        Ops::Empty => Error::Empty,
        Ops::NotSolid => Error::NotSolid,
        Ops::Exception => Error::Exception,
        Ops::NotOneFoot { found } => Error::NotOneFoot { found },
        Ops::Cancelled => Error::Cancelled(call),
        Ops::Internal(what) => Error::Internal(what),
        e => Error::Solidcore(match detail {
            Some(kernel) => format!("{call}: {kernel}"),
            None => format!("{call}: {e}"),
        }),
    }
}
