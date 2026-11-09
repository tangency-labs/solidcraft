//! Stopping the kernel calls of work nobody waits for any more: the jobs of a design that closed.
//! Work runs [`Stop::within`] its design's token; once the token is stopped, each operation under
//! it refuses to start, and one running stops when solidcore next checks ([`solidcore::kernel::Kernel::cancel`]).
//!
//! A stopped operation is an error like any other, and the caller's bodies are as they were: only
//! operations that build on copies stop. Calls that answer from a body itself ([`crate::core::with_body`],
//! measuring, say) always run to their end, since solidcore may leave what a stopped call borrows
//! half-changed.

use std::cell::RefCell;

use solidcore::kernel::CancelToken;

thread_local! {
    static CURRENT: RefCell<Option<Stop>> = const { RefCell::new(None) };
}

/// Stops the kernel work running under it, on every thread it reached.
#[derive(Clone, Debug, Default)]
pub struct Stop(CancelToken);

impl Stop {
    pub fn new() -> Stop {
        Stop::default()
    }

    /// Stop the work under this token, now and from now on.
    pub fn stop(&self) {
        self.0.cancel();
    }

    pub fn is_stopped(&self) -> bool {
        self.0.is_cancelled()
    }

    /// Run `f` with this token over the kernel work it does on this thread.
    pub fn within<R>(&self, f: impl FnOnce() -> R) -> R {
        scoped(Some(self.clone()), f)
    }
}

/// The token over this thread's work, to hand to the worker threads it starts.
pub fn current() -> Option<Stop> {
    CURRENT.with(|c| c.borrow().clone())
}

/// Run `f` under `stop` (`None`: under none), as [`current`] was on the thread that started it.
pub fn scoped<R>(stop: Option<Stop>, f: impl FnOnce() -> R) -> R {
    let previous = CURRENT.with(|c| c.replace(stop));
    struct Restore(Option<Option<Stop>>);
    impl Drop for Restore {
        fn drop(&mut self) {
            if let Some(previous) = self.0.take() {
                CURRENT.with(|c| *c.borrow_mut() = previous);
            }
        }
    }
    let _restore = Restore(Some(previous));
    f()
}

/// Run `f` so that no token stops it: for what fills a cell every later user of a body reads,
/// which must not keep a stopped job's error.
pub fn unstoppable<R>(f: impl FnOnce() -> R) -> R {
    scoped(None, f)
}

/// solidcore's token of the work on this thread.
pub(crate) fn token() -> Option<CancelToken> {
    CURRENT.with(|c| c.borrow().as_ref().map(|s| s.0.clone()))
}
