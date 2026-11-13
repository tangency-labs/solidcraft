//! Stopping the kernel calls of work nobody waits for any more: the jobs of a design that closed,
//! a feature whose dialog was cancelled (#10577). Work runs [`Stop::within`] its token; once the
//! token is stopped, each operation under it refuses to start, and one running stops when
//! solidcore next checks ([`solidcore::kernel::Kernel::cancel`]).
//!
//! Tokens nest: work entered under one token while another is over it runs under a token of its
//! own that stops when either does. A design's job a dialog started stops when the dialog is
//! cancelled and when the design closes.
//!
//! A stopped operation is an error like any other, and the caller's bodies are as they were: only
//! operations that build on copies stop. Calls that answer from a body itself ([`crate::core::with_body`],
//! measuring, say) always run to their end, since solidcore may leave what a stopped call borrows
//! half-changed.

use std::cell::RefCell;
use std::sync::{Arc, Mutex, Weak};

use solidcore::kernel::CancelToken;

thread_local! {
    static CURRENT: RefCell<Option<Stop>> = const { RefCell::new(None) };
}

/// Stops the kernel work running under it, on every thread it reached.
#[derive(Clone, Debug, Default)]
pub struct Stop(Arc<Token>);

#[derive(Debug, Default)]
struct Token {
    cancel: CancelToken,
    /// The tokens of work entered under this one while another was over it: they stop with it.
    /// Weak, so a token that work no longer holds goes; the dead ones are dropped as others come.
    linked: Mutex<Vec<Weak<Token>>>,
}

impl Token {
    fn stop(&self) {
        self.cancel.cancel();
        let linked = self.linked.lock().map(|l| l.iter().filter_map(Weak::upgrade).collect::<Vec<_>>()).unwrap_or_default();
        for token in linked {
            token.stop();
        }
    }

    fn link(&self, token: &Arc<Token>) {
        if let Ok(mut linked) = self.linked.lock() {
            linked.retain(|t| t.strong_count() > 0);
            linked.push(Arc::downgrade(token));
        }
    }
}

impl Stop {
    pub fn new() -> Stop {
        Stop::default()
    }

    /// Stop the work under this token, now and from now on, and the work entered under it
    /// beneath another token.
    pub fn stop(&self) {
        self.0.stop();
    }

    pub fn is_stopped(&self) -> bool {
        self.0.cancel.is_cancelled()
    }

    /// Run `f` with this token over the kernel work it does on this thread. Under another token
    /// already, `f` runs under one that stops when either does.
    pub fn within<R>(&self, f: impl FnOnce() -> R) -> R {
        let over = current().filter(|over| !Arc::ptr_eq(&over.0, &self.0));
        let stop = match over {
            Some(over) => Stop::either(&over, self),
            None => self.clone(),
        };
        scoped(Some(stop), f)
    }

    /// A token that stops when `a` or `b` does: linked first, then checked, so a stop between
    /// the two is not missed.
    fn either(a: &Stop, b: &Stop) -> Stop {
        let both = Stop::new();
        a.0.link(&both.0);
        b.0.link(&both.0);
        if a.is_stopped() || b.is_stopped() {
            both.stop();
        }
        both
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
    CURRENT.with(|c| c.borrow().as_ref().map(|s| s.0.cancel.clone()))
}
