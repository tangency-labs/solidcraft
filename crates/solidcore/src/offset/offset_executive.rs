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
mod crossing_loops;
mod edgeuse;
mod face;
mod merge;
pub mod p1;
pub mod p2;
mod step_gaps;
mod vertex;

// Supported public paths; numbered modules are implementation partitions.
pub use p2::{
    api_offset_brep_full, api_shell_brep_full, OffsetNotifications, OffsetOptions,
    ShellCopyIdentities, ShellOptions,
};
