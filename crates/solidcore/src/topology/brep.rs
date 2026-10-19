//! The B-rep object (`SmBrep`): construction from serialized data, editing and
//! queries, split into the units `topology.brep_p1` .. `topology.brep_p10`.
//!
//! Upstream: `source/SMLib/src/SmBrep.cpp`, `source/SMLib/inc/SmBrep.h`.
//!
//! | Rust      | Upstream                                                        |
//! |-----------|-----------------------------------------------------------------|
//! | [`p1`]    | see the module                                                  |
//! | [`p5`]    | B-rep editing and transformation operations, part 5             |
//! | [`p6`]    | see the module                                                  |
//! | [`p7`]    | see the module                                                  |
//! | [`p10`]   | `MakeTopologyFromData`, `ProjectAndTrim`, `RemoveAllTopology`, `TestClosureSplitRegion` (edge version) |
//!
//! Deviations: the B-rep records are the arenas of [`crate::topology::Brep`].

pub mod p1;
pub mod p10;
pub mod p5;
pub mod p6;
pub mod p7;
pub mod p9;
