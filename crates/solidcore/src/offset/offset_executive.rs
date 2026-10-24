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

mod edgeuse;
mod face;
pub mod p1;
pub mod p2;
mod vertex;
