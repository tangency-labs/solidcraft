//! The replay seam a test installs on its thread: [`crate::runner_kernel`] gives each
//! runner's kernel that seam ([`solidcore::Kernel::replay`], `solidcore`'s
//! `surface::surface::replay`, feature `wr57-replay`, which this crate turns on for its
//! tests only). The WR-450 replay ([`crate::wr450_rows`]), the WR-587 replay
//! ([`crate::wr587_replay`]) and the target bound's tests
//! (`units::intersect_curve_class_p1`) install their hooks here.

use std::cell::RefCell;
use std::collections::BTreeMap;

use serde_json::Value;
use solidcore::surface::surface::replay::{Answer, Call, ReplaySeam};
use solidcore::Kernel;

use crate::case::{repo_root, Case, CaseContext};
use crate::golden::encode;
use crate::UnitDiff;

/// Generated cases per unit, as `goldens` runs them (`main.rs` `DEFAULT_SEEDS`).
const SEEDS: usize = 16;

thread_local! {
    /// The seam a test installed on this thread, for [`with_seam`].
    static SEAM: RefCell<Option<ReplaySeam>> = const { RefCell::new(None) };
}

/// `k` with the seam a test installed on this thread, if any ([`crate::runner_kernel`]).
pub(crate) fn with_seam(mut k: Kernel) -> Kernel {
    SEAM.with(|s| {
        if let Some(seam) = s.borrow().as_ref() {
            k.replay = seam.clone();
        }
    });
    k
}

/// Whether a test has a seam installed on this thread ([`crate::replay_active`]).
pub(crate) fn active() -> bool {
    SEAM.with(|s| s.borrow().is_some())
}

/// Runs `f` with `seam` installed as this thread's seam.
/// The seam it replaced is restored when `f` returns or unwinds.
pub(crate) fn with_installed<R>(seam: ReplaySeam, f: impl FnOnce() -> R) -> R {
    struct Restore(Option<ReplaySeam>);
    impl Drop for Restore {
        fn drop(&mut self) {
            let previous = self.0.take();
            SEAM.with(|s| *s.borrow_mut() = previous);
        }
    }
    let _restore = Restore(SEAM.with(|s| s.borrow_mut().replace(seam)));
    f()
}

/// Runs `f` with `hook` installed as this thread's seam; the seam it replaced (an outer
/// hook, or none) is restored when `f` returns or unwinds ([`with_installed`]).
pub(crate) fn with_hook<R>(
    hook: impl FnMut(Call<'_>) -> Answer + Send + 'static,
    f: impl FnOnce() -> R,
) -> R {
    with_installed(ReplaySeam::new(Box::new(hook)), f)
}

/// The registered unit `id`.
pub(crate) fn unit(id: &str) -> Box<dyn UnitDiff> {
    crate::registry()
        .into_iter()
        .find(|u| u.id() == id)
        .unwrap_or_else(|| panic!("no unit {id}"))
}

/// `u`'s cases by name, generated as `goldens` generates them.
pub(crate) fn cases(u: &dyn UnitDiff) -> BTreeMap<String, Case> {
    let ctx = CaseContext {
        root: repo_root(),
        seeds: SEEDS,
    };
    u.cases(&ctx)
        .unwrap()
        .into_iter()
        .map(|c| (c.name.clone(), c))
        .collect()
}

/// `case`'s encoded row from `u`.
pub(crate) fn run(u: &dyn UnitDiff, case: &Case) -> Value {
    let out = u.run_rust(case);
    encode(
        &case.name,
        &out.unwrap_or_else(|e| panic!("{}: {e:?}", case.name)),
    )
}
