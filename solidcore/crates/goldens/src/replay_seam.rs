//! The replay seam a test installs on its thread: [`crate::runner_kernel`] gives each
//! runner's kernel that seam ([`solidcore::Kernel::replay`], `solidcore`'s
//! `surface::surface::replay`, feature `wr57-replay`, which this crate turns on for its
//! tests only). The WR-587 replay ([`crate::wr587_replay`]) and the target bound's tests
//! (`units::intersect_curve_class_p1`) install their hooks here.

use std::cell::RefCell;

use solidcore::surface::surface::replay::ReplaySeam;
use solidcore::Kernel;

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
