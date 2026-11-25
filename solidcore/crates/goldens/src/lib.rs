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
#[cfg(test)]
pub(crate) mod certify;
pub mod compare;
pub mod constructor;
pub mod dump;
pub mod fixtures;
pub mod flat;
#[cfg(test)]
pub(crate) mod g1r_cert;
pub mod golden;
#[cfg(test)]
pub(crate) mod interval;
pub mod jobs;
pub mod json;
#[cfg(test)]
mod merged_vertex_rows;
#[cfg(test)]
mod operand_trim_rows;
pub mod output;
pub(crate) mod own_builder;
pub(crate) mod own_reader;
pub mod plan;
pub(crate) mod refit;
#[cfg(test)]
pub(crate) mod replay_seam;
pub mod rng;
pub mod runner;
pub mod script;
pub mod solidcraft;
pub mod soundness;
pub mod ssi;
pub mod step;
pub(crate) mod tolsub;
pub(crate) mod trim_hook;
#[cfg(test)]
pub(crate) mod trims_on_edges;
pub mod util;
#[cfg(test)]
pub(crate) mod wr450_rows;
#[cfg(test)]
pub(crate) mod wr493_rows;
#[cfg(test)]
mod wr587_replay;

pub use case::Case;
pub use case::CaseContext;
pub use case::CaseSource;
pub use compare::compare;
pub use compare::AbsFloor;
pub use compare::CaseDiff;
pub use compare::Mismatch;
pub use compare::REL_TOL;
pub use output::Deferred;
pub use output::Output;
pub use output::Topology;
pub use own_reader::read_source_file;
pub use rng::Rng;
pub use runner::RunOptions;
pub use runner::UnitDiff;

/// A runner's kernel: [`solidcore::Kernel::default`]. In this crate's tests it also
/// carries the replay seam a test has installed on this thread ([`replay_seam`]: the
/// WR-450 and WR-587 replays' and the target bound's; the seam state is the kernel's, and the
/// runners make their own kernels, so they take it from here).
pub(crate) fn runner_kernel() -> solidcore::Kernel {
    let k = solidcore::Kernel::default();
    #[cfg(test)]
    let k = replay_seam::with_seam(k);
    k
}

/// Whether a substitution replay seam is installed on this thread (only in this crate's
/// tests): a runner's cache of earlier results is bypassed then, since a cached result
/// was computed without the seam's answers.
pub(crate) fn replay_active() -> bool {
    #[cfg(test)]
    {
        replay_seam::active()
    }
    #[cfg(not(test))]
    {
        false
    }
}

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
