//! Cooperative cancellation of ops calls (solidcore#3247).
//!
//! An application runs its work [`within`] a [`CancelToken`]; every ops call that work makes on
//! this thread runs with the token on the session's kernel, whose long loops poll it. Once the
//! token is cancelled ([`CancelToken::cancel`], from any thread), the call running stops at the
//! kernel's next check and every call after it is refused before it starts, each with
//! [`crate::Error::Cancelled`].
//!
//! What a cancelled call leaves:
//! - **Bodies as they came in.** A call that edits a body puts it back as it was
//!   ([`crate::session`]'s rollback), a call that reads one changes nothing, and a call that
//!   consumes its operands (a boolean) consumed what the application handed it, as on any error.
//! - **No result.** A call during which the token was cancelled returns `Cancelled`, even if the
//!   kernel finished first: an application never has to ask whether a result arrived in time.
//! - **A fresh kernel.** The kernel the call ran on may hold half-built caches; the session
//!   replaces it, as after a panic.
//!
//! Work that starts threads hands them [`current`] and runs each [`within`] it. Scopes nest: work
//! within one token entered within another stops when either is cancelled.

use std::cell::RefCell;

pub use solidcore::kernel::CancelToken;

thread_local! {
    static CURRENT: RefCell<Option<CancelToken>> = const { RefCell::new(None) };
}

/// Runs `f` with `token` over the ops calls it makes on this thread. Within another token
/// already, `f` runs under one that is cancelled when either is.
pub fn within<R>(token: &CancelToken, f: impl FnOnce() -> R) -> R {
    let next = match current() {
        Some(over) => token.either(&over),
        None => token.clone(),
    };
    scoped(Some(next), f)
}

/// The token over this thread's ops calls, to hand to the threads its work starts.
#[must_use]
pub fn current() -> Option<CancelToken> {
    CURRENT.with(|c| c.borrow().clone())
}

/// Whether the token over this thread's ops calls is cancelled (`false` with none).
#[must_use]
pub fn is_cancelled() -> bool {
    CURRENT.with(|c| c.borrow().as_ref().is_some_and(CancelToken::is_cancelled))
}

/// Runs `f` with exactly `token` over this thread's ops calls (`None`: none), putting back the
/// one before after it, also on a panic.
fn scoped<R>(token: Option<CancelToken>, f: impl FnOnce() -> R) -> R {
    /// The token before, put back on drop.
    struct Restore(Option<CancelToken>);
    impl Drop for Restore {
        fn drop(&mut self) {
            let previous = self.0.take();
            CURRENT.with(|c| *c.borrow_mut() = previous);
        }
    }
    let _restore = Restore(CURRENT.with(|c| c.replace(token)));
    f()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scopes_nest_and_end() {
        assert!(current().is_none() && !is_cancelled());
        let (outer, inner) = (CancelToken::new(), CancelToken::new());
        within(&outer, || {
            assert!(!is_cancelled());
            within(&inner, || {
                outer.cancel();
                assert!(is_cancelled(), "the inner scope stops with the outer token");
                outer.reset();
                inner.cancel();
                assert!(is_cancelled());
            });
            assert!(
                !is_cancelled(),
                "the inner token does not stop the outer scope"
            );
        });
        assert!(current().is_none());
    }

    #[test]
    fn a_panic_ends_the_scope() {
        let token = CancelToken::new();
        let r = std::panic::catch_unwind(|| within(&token, || panic!("inside")));
        assert!(r.is_err());
        assert!(current().is_none());
    }
}
