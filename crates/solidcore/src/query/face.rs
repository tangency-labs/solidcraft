//! Face, shell and region queries, split by the owning port unit.
//!
//! Upstream: `source/SMLib/src/SmFace.cpp`, `source/SMLib/inc/SmFace.h`,
//! `source/SMLib/src/SmShell.cpp`, `source/SMLib/src/SmRegion.cpp`,
//! `source/SM_API/src/SmApiCurves.cpp`.
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`p1`] | unit `query.face_p1` |
//! | [`p2`] | unit `query.face_p2` |
//!
//! Deviations: methods become functions over arena ids; see the unit modules.

pub mod p1;
pub mod p2;
