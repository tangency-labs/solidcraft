//! `goldens`: runs every unit's cases on solidcore and checks each output against the
//! unit's golden, its committed expected output ([`golden`]).
//!
//! A case passes when its topology, integers and status are identical to the golden's and
//! every value agrees within [`REL_TOL`] relative (see [`compare`](mod@compare)).
//!
//! Adding a runner: drop a file in `src/units/<unit_id_with_underscores>.rs`
//! defining `pub struct Runner;` that implements [`UnitDiff`]. It is
//! registered automatically by `build.rs`. See `crates/goldens/README.md`.

pub mod case;
/// Input contracts shared by several unit runners' generators.
pub mod cases {
    pub mod blend;
    pub mod brep_stream;
    pub mod contract;
    pub mod topology_entities;
}
pub mod compare;
pub mod constructor;
pub mod fixtures;
pub mod golden;
pub mod jobs;
pub mod json;
pub mod output;
pub(crate) mod own_builder;
pub(crate) mod own_reader;
pub mod plan;
pub mod rng;
pub mod runner;
pub mod solidcraft;
pub mod soundness;
pub mod ssi;
pub mod step;
pub mod util;

pub use case::{Case, CaseContext, CaseSource};
pub use compare::{compare, AbsFloor, CaseDiff, Mismatch, REL_TOL};
pub use output::{Deferred, Output, Topology};
pub use rng::Rng;
pub use runner::{RunOptions, UnitDiff};

/// Error from the harness itself (bad fixture, missing file, ...), as opposed
/// to an operation's error.
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

/// Registered unit runners (one per file in `src/units/`).
pub mod units {
    include!(concat!(env!("OUT_DIR"), "/units.rs"));
}

/// All registered runners, sorted by unit id.
pub fn registry() -> Vec<Box<dyn UnitDiff>> {
    let mut v = units::registered();
    v.sort_by(|a, b| a.id().cmp(b.id()));
    v
}
