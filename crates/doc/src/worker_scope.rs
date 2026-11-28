//! The file scope an evaluation runs under, carried onto the worker threads it starts.
//!
//! The scope (`vfs::PathScope`, the folders a plugin session, an agent or a render batch may
//! read) lives in the io crate above this one, on the calling thread; a worker thread starts
//! with none. Reads made while evaluating (a sketch text's font file) ask that scope, so the
//! workers evaluation spreads over run inside the caller's. The io crate installs the hook the
//! first time a scope is set ([`set_carrier`]); without it nothing is confined and nothing is
//! carried.

use std::sync::{Arc, OnceLock};

/// Runs a closure inside the context captured on the calling thread.
pub type Carried = Arc<dyn Fn(&mut dyn FnMut()) + Send + Sync>;

static CARRIER: OnceLock<fn() -> Carried> = OnceLock::new();

/// Set how the calling thread's file scope is captured for its workers. The first one set stays.
pub fn set_carrier(capture: fn() -> Carried) {
    let _ = CARRIER.set(capture);
}

/// The calling thread's scope, to run worker items in with [`within`].
pub(crate) fn capture() -> Option<Carried> {
    CARRIER.get().map(|capture| capture())
}

/// `f` inside `carried`; `None` if the carrier did not run it (the item is then left to the
/// caller, which is in scope).
pub(crate) fn within<R>(carried: &Option<Carried>, f: impl FnOnce() -> R) -> Option<R> {
    let Some(carried) = carried else { return Some(f()) };
    let mut f = Some(f);
    let mut out = None;
    carried(&mut || {
        if let Some(f) = f.take() {
            out = Some(f());
        }
    });
    out
}
