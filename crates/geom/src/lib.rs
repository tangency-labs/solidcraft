//! Geometry foundation for SolidCraft: 2D/3D vectors, sketch planes, axis-aligned boxes,
//! triangle meshes with their measures (volume, area, centroid), and freeform surface maths
//! (B-spline basis and NURBS surfaces in [`bspline`], spline spaces and the operators that make
//! splines compatible in [`spline_space`], one-surface fills of closed loops in [`fill`],
//! surfaces through curve networks in [`network`], T-splines in [`tspline`]). Units are millimetres.
//!
//! This crate has no knowledge of sketches, documents or a modelling backend; every other crate
//! builds on it.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod aabb2;
mod arcfit;
pub mod blend;
pub mod brep;
pub mod bspline;
mod bvh;
pub mod cell;
mod chain;
pub mod compare;
pub mod drawing_measure;
pub mod drawing_sheet;
pub mod drawing_view;
pub mod env;
pub mod fill;
pub mod fill_general;
mod grid;
pub mod hlr;
pub mod ids;
pub mod inertia;
mod mesh;
pub mod mesh_distance;
pub mod network;
pub mod paper;
pub mod pixel_grid;
mod plane;
mod profile;
pub mod revision;
pub mod section;
pub mod simplify;
pub mod spline_space;
pub mod srgb;
pub mod table;
pub mod tess;
pub mod title_block;
pub mod tspline;
mod unit_vec3;
mod vec;

pub use aabb2::Aabb2;
pub use arcfit::MAX_FIT_PIECES;
pub use blend::hermite_bezier;
pub use bvh::{Bvh, MeshIndex};
pub use chain::{Chain, Closed, Ends, Open, Path, Planar, Ring};
pub use grid::Grid;
pub use mesh::{Aabb3, BoxedTriangle, InsideIndex, MassProperties, Mesh, MeshMeasure, find_close_pair};
pub use plane::Plane;
pub use profile::{Loop2, MAX_BEZIER_CTRL, MAX_BEZIER_DEGREE, MAX_DERIVATIVE_ORDER, Region2, Seg2, Similarity2, bezier, regions_apart};
pub use unit_vec3::UnitVec3;
pub use vec::{Vec2, Vec3};

/// Geometric tolerance for coincidence tests (mm).
pub const EPS: f64 = 1e-9;
/// Linear tolerance used for "same point" decisions in modelling (mm).
pub const LINEAR_TOL: f64 = 1e-6;

/// `x` if finite, else `fallback`.
pub fn finite_or(x: f64, fallback: f64) -> f64 {
    if x.is_finite() { x } else { fallback }
}

#[cfg(test)]
mod tests;
