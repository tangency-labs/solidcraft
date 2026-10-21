//! Per-case memo of C++ fixture exports, shared by the two sides of a case.
//!
//! Many runners build their B-rep from a C++ export (a shim's `Export` op on an `.smb`
//! fixture and an input block): `run_cpp` exports it before running the C++ operation,
//! and `run_rust` exports the same B-rep again to import it into Rust. The second export
//! repeats the first exactly (same shim, same paths, same block bits), and it used to be
//! most of the "Rust" time of those units. [`run_unit`](crate::run_unit) therefore lets
//! the Rust side reuse the C++ side's export:
//!
//! - while `run_cpp` runs, every [`export`] calls C++ exactly as before and records the
//!   result, so the C++ side makes the same calls in the same order;
//! - while `run_rust` runs, an [`export`] whose shim, paths and block bits match a
//!   recorded one returns a copy of it; any other export calls C++ (and is recorded);
//! - the memo is emptied after every case, so nothing is shared between cases;
//! - outside [`run_unit`](crate::run_unit) (fuzzing, tests, direct calls) the memo is off
//!   and [`export`] just calls C++.
//!
//! Failed exports are never recorded. Entries are found by key, so results never depend
//! on the order of the memo.

use std::cell::RefCell;

/// Which side of a case is running.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// No case is running: call through, record nothing.
    Off,
    /// `run_cpp`: call through and record.
    Cpp,
    /// `run_rust`: reuse a recorded export, else call through and record.
    Rust,
}

/// One recorded export.
struct Entry {
    shim: &'static str,
    paths: Vec<String>,
    block: Vec<f64>,
    export: Vec<f64>,
}

impl Entry {
    fn matches<P: AsRef<str>>(&self, shim: &str, paths: &[P], block: &[f64]) -> bool {
        self.shim == shim
            && self.paths.len() == paths.len()
            && self.paths.iter().zip(paths).all(|(a, b)| a == b.as_ref())
            && self.block.len() == block.len()
            && self
                .block
                .iter()
                .zip(block)
                .all(|(a, b)| a.to_bits() == b.to_bits())
    }
}

struct Memo {
    phase: Phase,
    entries: Vec<Entry>,
}

thread_local! {
    static MEMO: RefCell<Memo> = const {
        RefCell::new(Memo { phase: Phase::Off, entries: Vec::new() })
    };
}

fn set_phase(phase: Phase) {
    MEMO.with(|m| m.borrow_mut().phase = phase);
}

/// The C++ export `call()` of `paths` and `block` through the shim named `shim` (e.g.
/// `"topology_edgeuse_p1"`), reusing the C++ side's export of the same case when called
/// from `run_rust` (see the module docs). `call` must be that shim's export of exactly
/// these `paths` and `block`.
///
/// # Errors
///
/// The error of `call`.
pub fn export<P: AsRef<str>, E>(
    shim: &'static str,
    paths: &[P],
    block: &[f64],
    call: impl FnOnce() -> Result<Vec<f64>, E>,
) -> Result<Vec<f64>, E> {
    let phase = MEMO.with(|m| m.borrow().phase);
    if phase == Phase::Rust {
        let hit = MEMO.with(|m| {
            m.borrow()
                .entries
                .iter()
                .find(|e| e.matches(shim, paths, block))
                .map(|e| e.export.clone())
        });
        if let Some(export) = hit {
            return Ok(export);
        }
    }
    let export = call()?;
    if phase != Phase::Off {
        MEMO.with(|m| {
            m.borrow_mut().entries.push(Entry {
                shim,
                paths: paths.iter().map(|p| p.as_ref().to_string()).collect(),
                block: block.to_vec(),
                export: export.clone(),
            });
        });
    }
    Ok(export)
}

/// The memo's lifetime for one case: records from creation, serves the Rust side after
/// [`rust_side`], and is emptied and switched off when dropped.
pub(crate) struct CaseScope(());

impl CaseScope {
    /// Starts a case (the C++ side runs next).
    pub(crate) fn new() -> Self {
        MEMO.with(|m| {
            let mut m = m.borrow_mut();
            m.entries.clear();
            m.phase = Phase::Cpp;
        });
        Self(())
    }
}

/// The Rust side of the case in the current [`CaseScope`] runs next.
pub(crate) fn rust_side() {
    set_phase(Phase::Rust);
}

impl Drop for CaseScope {
    fn drop(&mut self) {
        MEMO.with(|m| {
            let mut m = m.borrow_mut();
            m.entries.clear();
            m.phase = Phase::Off;
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn counted(calls: &mut usize, v: f64) -> impl FnOnce() -> Result<Vec<f64>, ()> + '_ {
        move || {
            *calls += 1;
            Ok(vec![v])
        }
    }

    #[test]
    fn rust_side_reuses_the_cpp_export_of_the_same_case_only() {
        let mut calls = 0;
        // off: always calls through
        export("s", &["a"], &[1.0], counted(&mut calls, 1.0)).unwrap();
        export("s", &["a"], &[1.0], counted(&mut calls, 1.0)).unwrap();
        assert_eq!(calls, 2);
        {
            let _scope = CaseScope::new();
            // the C++ side always calls through, even for a repeat
            export("s", &["a"], &[1.0], counted(&mut calls, 1.0)).unwrap();
            export("s", &["a"], &[1.0], counted(&mut calls, 1.0)).unwrap();
            assert_eq!(calls, 4);
            rust_side();
            let r = export("s", &["a"], &[1.0], counted(&mut calls, 9.0)).unwrap();
            assert_eq!((r, calls), (vec![1.0], 4));
            // a different shim, path or block bit pattern is a miss
            export("t", &["a"], &[1.0], counted(&mut calls, 2.0)).unwrap();
            export("s", &["b"], &[1.0], counted(&mut calls, 2.0)).unwrap();
            export("s", &["a"], &[-0.0], counted(&mut calls, 2.0)).unwrap();
            export("s", &["a"], &[0.0], counted(&mut calls, 2.0)).unwrap();
            assert_eq!(calls, 8);
        }
        // the next case starts empty
        let _scope = CaseScope::new();
        export("s", &["a"], &[1.0], counted(&mut calls, 1.0)).unwrap();
        assert_eq!(calls, 9);
    }

    #[test]
    fn failed_exports_are_not_recorded() {
        let _scope = CaseScope::new();
        assert!(export::<_, ()>("s", &["a"], &[], || Err(())).is_err());
        rust_side();
        let mut calls = 0;
        export("s", &["a"], &[], counted(&mut calls, 1.0)).unwrap();
        assert_eq!(calls, 1);
    }
}
