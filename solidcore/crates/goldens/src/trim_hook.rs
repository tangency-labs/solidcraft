//! A hook on the result bodies of the units whose rows show UV trims (`boolean.merge_p4`,
//! `topology.brep_p7`, and `topology.face_p1`'s `MoveSeam`): each calls [`observe`] on a
//! body just before writing it (`MoveSeam` just after). Only tests install a hook
//! (`operand_trim_rows`, which blinds or certifies the bodies' trims; `wr469_replay`, which
//! reads the bodies' trims and level 2); with none, [`observe`] does nothing.

use solidcore::topology::Brep;
use std::cell::RefCell;

/// A hook run on each observed body.
pub(crate) type Hook = Box<dyn FnMut(&mut Brep)>;

thread_local! {
    static HOOK: RefCell<Option<Hook>> = const { RefCell::new(None) };
}

/// Runs this thread's hook, if one is installed, on `b`.
pub(crate) fn observe(b: &mut Brep) {
    HOOK.with(|h| {
        if let Some(f) = h.borrow_mut().as_mut() {
            f(b);
        }
    });
}

/// Runs `run` with `hook` installed on this thread.
#[cfg(test)]
pub(crate) fn with_hook<R>(hook: Hook, run: impl FnOnce() -> R) -> R {
    HOOK.with(|h| *h.borrow_mut() = Some(hook));
    let r = run();
    HOOK.with(|h| *h.borrow_mut() = None);
    r
}
