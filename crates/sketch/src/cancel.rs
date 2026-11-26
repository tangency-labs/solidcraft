//! Cancelling a sketch solve: the app's Cancel of a long evaluation, or the design it was for
//! closing. Work that may be cancelled runs [`within`] a token (the evaluation's stop token,
//! the one its kernel work polls); the solver checks it every iteration and its linear solve
//! every pivot, so a solve that would take minutes stops at once. A cancelled solve fails and
//! leaves the geometry as it was.
//!
//! The token is the sketch crate's own: entering one cancels nothing else (kernel calls keep
//! the token their own caller gave them).

use std::cell::RefCell;

pub use solidcraft_kernel::services::cancel::CancelToken;

thread_local! {
    static CURRENT: RefCell<Option<CancelToken>> = const { RefCell::new(None) };
}

/// Run `f` with `token` over the sketch solves it makes on this thread. Within another token
/// already, `f` runs under one that is cancelled when either is.
pub fn within<R>(token: &CancelToken, f: impl FnOnce() -> R) -> R {
    let next = match CURRENT.with(|c| c.borrow().clone()) {
        Some(over) => token.either(&over),
        None => token.clone(),
    };
    /// The token before, put back on drop (also on a panic).
    struct Restore(Option<CancelToken>);
    impl Drop for Restore {
        fn drop(&mut self) {
            let previous = self.0.take();
            CURRENT.with(|c| *c.borrow_mut() = previous);
        }
    }
    let _restore = Restore(CURRENT.with(|c| c.replace(Some(next))));
    f()
}

/// Whether the token over this thread's sketch solves is cancelled (`false` with none).
pub(crate) fn is_cancelled() -> bool {
    CURRENT.with(|c| c.borrow().as_ref().is_some_and(CancelToken::is_cancelled))
}
