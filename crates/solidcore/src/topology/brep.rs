//! B-rep construction and editing, grouped by porting unit.
//!
//! Upstream: `source/SMLib/inc/SmBrep.h`, `source/SMLib/src/SmBrep.cpp`.
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`p1`] | Tolerance accessors, topology collections, infinite region and region voting, part 1 |
//! | [`p5`] | B-rep editing and transformation operations, part 5 |
//! | [`p6`] | B-rep construction and editing, part 6 |
//! | [`p7`] | B-rep reading, face construction and copying, part 7 |
//!
//! Deviations: the arena record remains [`super::Brep`]. Supporting notification
//! tracking integration is documented in part 5 and `plan/modules.toml`.

pub mod p1;
pub mod p5;
pub mod p6;
pub mod p7;
pub mod p9;
