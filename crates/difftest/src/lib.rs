//! `difftest`: side-by-side comparison of solidcore (Rust) against upstream
//! SMLib (C++ via [`solidcore_ffi`]) on identical inputs.
//!
//! A unit is green only when, for every case, the topology is identical and
//! every value agrees within [`REL_TOL`] relative (see [`compare`]).
//!
//! Adding a runner: drop a file in `src/units/<unit_id_with_underscores>.rs`
//! defining `pub struct Runner;` that implements [`UnitDiff`]. It is
//! registered automatically by `build.rs`. See `crates/difftest/README.md`.

pub mod case;
pub mod compare;
pub mod export_memo;
pub mod fixtures;
pub mod fuzz;
pub mod jobs;
pub mod json;
pub mod output;
pub mod plan;
pub mod report;
pub mod rng;
pub mod runner;
pub mod solidcraft;
pub mod ssi;
pub mod util;

pub use case::{Case, CaseContext, CaseSource};
pub use compare::{compare, AbsFloor, CaseDiff, Mismatch, REL_TOL};
pub use output::{Deferred, Output, Topology};
pub use rng::Rng;
pub use runner::{run_unit, CaseOutcome, RunOptions, UnitDiff, UnitResult, UnitStatus, UnitTiming};

/// Error from the harness itself (bad fixture, missing file, ...), as opposed
/// to a comparison failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HarnessError(pub String);

impl std::fmt::Display for HarnessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for HarnessError {}

impl HarnessError {
    /// Convenience constructor.
    pub fn new(msg: impl Into<String>) -> Self {
        Self(msg.into())
    }
}

/// Registered unit runners (one per file in `src/units/`, only with feature `cpp`).
pub mod units {
    include!(concat!(env!("OUT_DIR"), "/units.rs"));
}

/// All registered runners, sorted by unit id.
pub fn registry() -> Vec<Box<dyn UnitDiff>> {
    let mut v = units::registered();
    v.sort_by(|a, b| a.id().cmp(b.id()));
    v
}
