//! Geometry as data (docs/layers.md, "Record"): the portable B-rep record with its serde form
//! and structural validation ([`brep`]), its history and metadata records, the native body
//! file ([`native`]), and the plain data of vectors ([`Vec2`], [`Vec3`]), sketch planes
//! ([`Plane`]) and planar profiles ([`Seg2`], [`Loop2`], [`Region2`]), and STEP's Part 21
//! exchange structure as parsed from a file with the units of its representation contexts and
//! its Part 42 curves and surfaces ([`step`]).
//!
//! The record computes no geometric quantity: no square roots, trigonometry, dot or cross
//! products, normalising or solves (clippy `disallowed-methods`, this crate's `clippy.toml`). It
//! depends on no geometry or maths crate. Evaluating, measuring and checking the geometry a
//! record holds is the kernel's.
#![forbid(unsafe_code)]
#![deny(clippy::disallowed_methods)]

// The record's types are SolidCraft's wire format, carried over as it is: each type says what it
// is, its formula and its SMLib and STEP counterparts, and its fields are the names in that
// formula (SolidCraft's docs/native-brep.md), so fields and variants are not documented one by
// one.
#[allow(missing_docs)]
pub mod brep;
pub mod native;
pub mod plane;
// Profiles are SolidCraft's sketch wire format, carried over as it is: each variant says what it
// is and its fields are the names in its formula.
#[allow(missing_docs)]
pub mod profile;
pub mod step;
pub mod vec;

pub use plane::Plane;
pub use profile::{Loop2, MAX_BEZIER_CTRL, MAX_BEZIER_DEGREE, Region2, Seg2};
pub use vec::{Vec2, Vec3};
