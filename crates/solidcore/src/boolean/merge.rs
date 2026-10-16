//! The merge (boolean) engine `SmMerge`: intersecting and combining the topologies of
//! two B-reps.
//!
//! Upstream: `source/SMLib/src/SmMerge.cpp`, `source/SMLib/inc/SmMerge.h`.
//! The file is split over several plan units (`boolean.merge_p1`, `_p2`, ...); each
//! lives in its own submodule. The wire/shell-vertex stage is in [`p5`]; the merge
//! options were CO-PORTED by `boolean.api_brep`.

pub mod p1;
pub mod p5;

pub use p1::MergeOptions;
