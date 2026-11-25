//! Repeated LM runs inside one edit's solve. An edit that the one-jump solve cannot reach is
//! stepped there by continuation, and when that fails, by [`super::solve_in_steps`] in 2, 4, 8
//! and 16 steps: each try starts from the same stored state, and its first sub-steps are runs the
//! tries before it already made, on the same values from the same start. On a sketch whose edit
//! fails that way, those repeats were most of the time (minutes before the solve was sparse).
//!
//! Within [`scope`], a run of a large component is looked up by everything it reads (the
//! sketch, the layout, its blocks with their values, its variables and the whole starting point,
//! bit for bit, its iteration cap and whether it gives up when stalled) and, when it was made
//! before, its result is taken again: the same values, so the solve is the same.

use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::{DefaultHasher, Hasher};

/// A run's result: its variables' final values and what LM returned.
pub(super) struct Entry {
    pub(super) values: Vec<f64>,
    pub(super) out: Option<(usize, f64)>,
}

/// Runs remembered at most in one scope; past that, new runs are not kept (each is a few
/// kilobytes).
const MAX_ENTRIES: usize = 512;

thread_local! {
    static MEMO: RefCell<Option<HashMap<[u64; 2], Entry>>> = const { RefCell::new(None) };
}

/// Run `f` remembering the runs it makes (nested scopes share the outer one).
pub(super) fn scope<R>(f: impl FnOnce() -> R) -> R {
    if active() {
        return f();
    }
    /// Forgets the runs on drop (also on a panic).
    struct Forget;
    impl Drop for Forget {
        fn drop(&mut self) {
            MEMO.with(|m| *m.borrow_mut() = None);
        }
    }
    MEMO.with(|m| *m.borrow_mut() = Some(HashMap::new()));
    let _forget = Forget;
    f()
}

pub(super) fn active() -> bool {
    MEMO.with(|m| m.borrow().is_some())
}

/// Whether runs of a component of `n` unknowns are remembered: large ones, whose runs cost
/// enough to look up (in this crate's tests, any size inside [`tests::any_size`]).
pub(super) fn remembers(n: usize) -> bool {
    #[cfg(test)]
    if tests::ANY_SIZE.with(std::cell::Cell::get) {
        return true;
    }
    n >= super::SPARSE_MIN_VARS
}

/// The remembered result of the run with `key`.
pub(super) fn get(key: &[u64; 2]) -> Option<(Vec<f64>, Option<(usize, f64)>)> {
    let hit = MEMO.with(|m| m.borrow().as_ref().and_then(|m| m.get(key)).map(|e| (e.values.clone(), e.out)));
    #[cfg(test)]
    if hit.is_some() {
        tests::HITS.with(|h| h.set(h.get() + 1));
    }
    hit
}

pub(super) fn put(key: [u64; 2], entry: Entry) {
    MEMO.with(|m| {
        if let Some(m) = m.borrow_mut().as_mut()
            && m.len() < MAX_ENTRIES
        {
            m.insert(key, entry);
        }
    });
}

/// Two independent 64-bit hashes of the same bytes: a 128-bit key.
pub(super) struct Key(DefaultHasher, DefaultHasher);

impl Key {
    pub(super) fn new() -> Key {
        let mut second = DefaultHasher::new();
        second.write(b"second hash of an LM run");
        Key(DefaultHasher::new(), second)
    }
    pub(super) fn bytes(&mut self, b: &[u8]) {
        self.0.write(b);
        self.1.write(b);
    }
    pub(super) fn usize(&mut self, v: usize) {
        self.bytes(&(v as u64).to_le_bytes());
    }
    pub(super) fn f64(&mut self, v: f64) {
        self.bytes(&v.to_bits().to_le_bytes());
    }
    pub(super) fn finish(self) -> [u64; 2] {
        [self.0.finish(), self.1.finish()]
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::cell::Cell;

    thread_local! {
        pub(super) static ANY_SIZE: Cell<bool> = const { Cell::new(false) };
        pub(super) static HITS: Cell<usize> = const { Cell::new(0) };
    }

    /// Run `f` remembering the runs of components of any size; with the runs it took again.
    pub(crate) fn any_size<R>(f: impl FnOnce() -> R) -> (R, usize) {
        ANY_SIZE.with(|a| a.set(true));
        HITS.with(|h| h.set(0));
        let r = f();
        ANY_SIZE.with(|a| a.set(false));
        (r, HITS.with(Cell::get))
    }
}
