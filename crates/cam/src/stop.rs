//! Stopping long planning from another thread: a caller hands a flag to [`scoped`], and the long
//! loops that already count their work (the drop-cutter's samples, the adaptive clearing's work)
//! look at it as they count. A stop set while they run ends them with [`CamError::Stopped`],
//! never with half a toolpath. Work handed to other threads carries the flag along with
//! [`current`].

use std::cell::RefCell;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::{CamError, Result};

thread_local! {
    static STOP: RefCell<Option<Arc<AtomicBool>>> = const { RefCell::new(None) };
}

/// Puts the flag back as it was when the scope ends, panics included.
struct Restore(Option<Arc<AtomicBool>>);

impl Drop for Restore {
    fn drop(&mut self) {
        let prev = self.0.take();
        STOP.with(|s| *s.borrow_mut() = prev);
    }
}

/// Runs `f` with `stop` as this thread's stop flag (none: it cannot be stopped).
pub fn scoped<R>(stop: Option<Arc<AtomicBool>>, f: impl FnOnce() -> R) -> R {
    let prev = STOP.with(|s| s.replace(stop));
    let _restore = Restore(prev);
    f()
}

/// This thread's stop flag, to hand to threads it starts.
pub fn current() -> Option<Arc<AtomicBool>> {
    STOP.with(|s| s.borrow().clone())
}

/// Whether this thread's work was asked to stop.
pub fn stopped() -> bool {
    STOP.with(|s| s.borrow().as_ref().is_some_and(|a| a.load(Ordering::Relaxed)))
}

/// [`CamError::Stopped`] when this thread's work was asked to stop.
pub fn check() -> Result<()> {
    if stopped() { Err(CamError::Stopped) } else { Ok(()) }
}
