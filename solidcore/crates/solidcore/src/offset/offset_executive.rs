//! Topological offset executive, split by callgraph ownership.
//!
//! Upstream: `source/SMLib/inc/SmOffsetExecutive.h`,
//! `source/SMLib/src/SmOffsetExecutive.cpp`.
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`p1::OffsetExecutive`] | `SmOffsetExecutive` |
//!
//! Deviations: typed arena ids replace pointers; each part documents its dependency boundaries.

mod attributes;
mod cavity;
pub(crate) mod clearance;
mod crossing_loops;
mod edgeuse;
mod face;
mod merge;
pub mod p1;
pub mod p2;
pub mod reach;
mod step_gaps;
mod vertex;

// Supported public paths; numbered modules are implementation partitions.
pub use p2::{
    api_offset_brep_full, api_shell_brep_full, OffsetNotifications, OffsetOptions,
    ShellCopyIdentities, ShellOptions,
};
// (issue #13622's hook) the gap pass's deferred-vanish refusal, replayed by the
// crossed-boundary reconstruction (`crate::topology::brep::local_operation`) where its
// discharge does not take the event.
pub(crate) use step_gaps::deferred_edge_vanish;
