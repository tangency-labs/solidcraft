//! Fuel: a deterministic work budget for ops calls, so a call that is going nowhere stops early
//! with [`crate::Error::OutOfFuel`] instead of running for minutes and failing anyway.
//!
//! - **What it counts.** One unit is one poll of the kernel's checks at its outer loops: the
//!   steps of intersection marching and tracing, the fillet solver and its corners, the
//!   boolean's classification and merge, offset, stitch and heal, integration. These are the
//!   same checks [`crate::cancel`] stops at. A count of work, never of time, so a call spends
//!   the same fuel on every machine, under load, in CI and in a debug build, and a limit
//!   stops it at the same point everywhere.
//! - **Nothing else changes.** A check only counts; no computation reads the count. A call
//!   that finishes within its fuel returns exactly what it returned without fuel.
//! - **Running out** stops the call at the kernel's next check. The call answers
//!   [`crate::Error::OutOfFuel`], never a partial body passed off as a result: like a
//!   cancelled call, a call whose fuel ran out returns no result even if the kernel finished,
//!   and puts back the body it was editing ([`crate::session`]).
//! - **Defaults:** none yet. Every call runs without a limit unless its caller sets one with
//!   [`within`]; [`DEFAULTS`] is empty. A valid STEP part read back as sheet metal spent more
//!   than ten times the most any corpus case had spent in one call, so a fixed per-call
//!   limit would refuse valid work. Limits relative to the size of the input come later, from
//!   measurement (tangency-labs/solidcore#3390).
//! - **Callers** change it for the calls made inside [`within`]: [`Fuel::Times`] for a
//!   "try harder" retry (no effect until calls have defaults), [`Fuel::Limit`] for an exact
//!   budget, [`Fuel::Unlimited`] for none.
//!   Work that starts threads hands them [`current`] and runs each [`within`] it.

use std::cell::Cell;

/// The fuel the ops calls made [`within`] it run with.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Fuel {
    /// Each call's default ([`default_limit`]).
    #[default]
    Default,
    /// `n` times each call's default, for a retry that tries harder (a call without a default
    /// still runs without a limit).
    Times(u64),
    /// Exactly this many work units for each call.
    Limit(u64),
    /// No limit.
    Unlimited,
}

impl Fuel {
    /// The work units `call` may spend under this fuel; `None`: no limit.
    #[must_use]
    pub fn limit(self, call: &str) -> Option<u64> {
        match self {
            Fuel::Default => default_limit(call),
            Fuel::Times(n) => default_limit(call).map(|d| d.saturating_mul(n)),
            Fuel::Limit(n) => Some(n),
            Fuel::Unlimited => None,
        }
    }
}

/// Each call's default fuel, by its session call name (the name [`crate::FailureCode::call`]
/// and [`crate::session::current_call`] give), kept sorted by name. A call left out runs
/// without a limit. Empty: no call has a default yet. A fixed limit per call is not enough;
/// a valid STEP part spent 1,746,027 units in one `planar_regions_boolean`, more than ten
/// times the corpus's most (171,864). Defaults relative to the input's size come with their
/// measurements (tangency-labs/solidcore#3390).
const DEFAULTS: &[(&str, u64)] = &[];

/// The default fuel of `call` ([`DEFAULTS`]); `None` for a call with none measured, which runs
/// without a limit.
#[must_use]
pub fn default_limit(call: &str) -> Option<u64> {
    DEFAULTS
        .binary_search_by(|(name, _)| (*name).cmp(call))
        .ok()
        .and_then(|i| DEFAULTS.get(i))
        .map(|&(_, n)| n)
}

thread_local! {
    static CURRENT: Cell<Fuel> = const { Cell::new(Fuel::Default) };
}

/// Runs `f` with `fuel` over the ops calls it makes on this thread, putting back the fuel before
/// after it, also on a panic. An inner scope replaces an outer one.
pub fn within<R>(fuel: Fuel, f: impl FnOnce() -> R) -> R {
    /// The fuel before, put back on drop.
    struct Restore(Fuel);
    impl Drop for Restore {
        fn drop(&mut self) {
            let previous = self.0;
            CURRENT.with(|c| c.set(previous));
        }
    }
    let _restore = Restore(CURRENT.with(|c| c.replace(fuel)));
    f()
}

/// The fuel over this thread's ops calls, to hand to the threads its work starts.
#[must_use]
pub fn current() -> Fuel {
    CURRENT.with(Cell::get)
}

/// The work units `call` may spend on this thread now ([`current`]'s [`Fuel::limit`]).
pub(crate) fn limit(call: &str) -> Option<u64> {
    current().limit(call)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_sorted_and_positive() {
        assert!(
            DEFAULTS.windows(2).all(|w| w[0].0 < w[1].0),
            "DEFAULTS sorted, no repeats"
        );
        assert!(DEFAULTS.iter().all(|&(_, n)| n > 0));
    }

    #[test]
    fn scopes_set_and_restore() {
        assert_eq!(current(), Fuel::Default);
        within(Fuel::Limit(7), || {
            assert_eq!(limit("any call"), Some(7));
            within(Fuel::Unlimited, || assert_eq!(limit("any call"), None));
            assert_eq!(current(), Fuel::Limit(7));
        });
        assert_eq!(current(), Fuel::Default);
        let r = std::panic::catch_unwind(|| within(Fuel::Limit(1), || panic!("inside")));
        assert!(r.is_err());
        assert_eq!(current(), Fuel::Default);
    }

    #[test]
    fn times_scales_only_a_default() {
        assert_eq!(Fuel::Times(10).limit("no such call"), None);
        assert_eq!(Fuel::Default.limit("no such call"), None);
        // no call has a default yet: the boolean and the planar region boolean run unlimited
        assert_eq!(Fuel::Default.limit("SmApiBoolean"), None);
        assert_eq!(Fuel::Default.limit("planar_regions_boolean"), None);
        if let Some(&(call, n)) = DEFAULTS.first() {
            assert_eq!(Fuel::Times(3).limit(call), Some(n * 3));
        }
    }
}
