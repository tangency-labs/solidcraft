//! The merge (boolean) engine `SmMerge`: intersecting and combining the topologies of
//! two B-reps.
//!
//! Upstream: `source/SMLib/src/SmMerge.cpp`, `source/SMLib/inc/SmMerge.h`.
//! The file is split over several plan units (`boolean.merge_p1`, `_p2`, ...); each
//! lives in its own submodule. Only the merge options are ported so far (CO-PORTED by
//! `boolean.api_brep`).

pub mod p1;

pub use p1::MergeOptions;
