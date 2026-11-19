//! A test seam at the runners' file reads: after a reader has built and healed a B-rep,
//! [`after_read`] lets this crate's tests record the edge and vertex tolerances or put
//! other bits in their place. Outside `cfg(test)` it does nothing.
//!
//! The tolerance-substitution replay (`tolsub::replay`, #18230) uses it to show
//! that a golden row moved only through the tolerances a read set: with main's tolerance
//! bits put back on the entities the read changed, the row is main's row bit for bit.
//!
//! The seams are the readers the runners share ([`crate::own_reader`]), `corpus.parity`'s
//! `read_breps` and `topology.brep_p10`'s `make_topology_from_data` cases. Each call is one
//! read, numbered from 0 within a case in call order. While a mode is installed
//! ([`active`]), `topology.face_p1`'s cache of recent reads is bypassed.

use solidcore::topology::Brep;

/// Called by every read seam with the B-rep the reader returned, before the runner uses it.
pub(crate) fn after_read(b: &mut Brep) {
    #[cfg(test)]
    state::after_read(b);
    #[cfg(not(test))]
    let _ = b;
}

/// Whether a test has installed a mode: readers that cache a read then read afresh, so
/// the seam sees every read. Always `false` outside `cfg(test)`.
#[cfg(test)]
pub(crate) fn active() -> bool {
    state::active()
}

/// [`active`] outside `cfg(test)`.
#[cfg(not(test))]
pub(crate) fn active() -> bool {
    false
}

#[cfg(test)]
mod replay;

#[cfg(test)]
pub(crate) mod state {
    use std::cell::RefCell;

    use solidcore::topology::Brep;

    /// One read's tolerances: `(arena index, bits)` per face, edge and vertex.
    #[derive(Debug, Default, Clone, serde::Serialize)]
    pub(crate) struct Snapshot {
        pub faces: Vec<(u32, u64)>,
        pub edges: Vec<(u32, u64)>,
        pub vertices: Vec<(u32, u64)>,
    }

    /// One substitution: on read `read`, the edge (`vertex == false`) or vertex `index`
    /// must hold `now` (this branch's read) and gets `main`; one that holds `main` already
    /// is left as it is.
    #[derive(Debug, Clone, Copy)]
    pub(crate) struct Sub {
        pub read: u32,
        pub vertex: bool,
        pub index: u32,
        pub now: u64,
        pub main: u64,
    }

    /// What the seam does.
    pub(crate) enum Mode {
        /// Append each read's snapshot.
        Record(Vec<Snapshot>),
        /// Apply the substitutions of each read, checking the value each replaces.
        Substitute {
            subs: Vec<Sub>,
            reads: u32,
            applied: usize,
            /// Of `applied`, those that held `main` already.
            already: usize,
            failures: Vec<String>,
        },
    }

    thread_local! {
        static MODE: RefCell<Option<Mode>> = const { RefCell::new(None) };
    }

    /// Installs `mode` for the reads that follow on this thread.
    pub(crate) fn set(mode: Option<Mode>) {
        MODE.with(|m| *m.borrow_mut() = mode);
    }

    /// Whether a mode is installed.
    pub(crate) fn active() -> bool {
        MODE.with(|m| m.borrow().is_some())
    }

    /// Removes and returns the installed mode.
    pub(crate) fn take() -> Option<Mode> {
        MODE.with(|m| m.borrow_mut().take())
    }

    pub(super) fn after_read(b: &mut Brep) {
        MODE.with(|m| match m.borrow_mut().as_mut() {
            None => {}
            Some(Mode::Record(v)) => v.push(snapshot(b)),
            Some(Mode::Substitute {
                subs,
                reads,
                applied,
                already,
                failures,
            }) => {
                let read = *reads;
                *reads += 1;
                for s in subs.iter().filter(|s| s.read == read) {
                    let slot = if s.vertex {
                        let id = b
                            .vertices()
                            .iter()
                            .map(|(id, _)| id)
                            .find(|id| id.index() == s.index);
                        id.and_then(|id| b.vertex_mut(id).ok())
                            .map(|v| &mut v.tolerance)
                    } else {
                        let id = b
                            .edges()
                            .iter()
                            .map(|(id, _)| id)
                            .find(|id| id.index() == s.index);
                        id.and_then(|id| b.edge_mut(id).ok())
                            .map(|e| &mut e.tolerance)
                    };
                    match slot {
                        Some(t) if t.to_bits() == s.now => {
                            *t = f64::from_bits(s.main);
                            *applied += 1;
                        }
                        // (a read of a body written after an earlier substitution, as
                        // `corpus.parity`'s round trip reads back, already holds main's)
                        Some(t) if t.to_bits() == s.main => {
                            *applied += 1;
                            *already += 1;
                        }
                        Some(t) => failures.push(format!("{s:?}: holds {:e}", *t)),
                        None => failures.push(format!("{s:?}: no such entity")),
                    }
                }
            }
        });
    }

    /// The face, edge and vertex tolerances of `b`.
    pub(crate) fn snapshot(b: &Brep) -> Snapshot {
        Snapshot {
            faces: b
                .faces()
                .iter()
                .map(|(id, f)| (id.index(), f.tolerance.to_bits()))
                .collect(),
            edges: b
                .edges()
                .iter()
                .map(|(id, e)| (id.index(), e.tolerance.to_bits()))
                .collect(),
            vertices: b
                .vertices()
                .iter()
                .map(|(id, v)| (id.index(), v.tolerance.to_bits()))
                .collect(),
        }
    }
}
