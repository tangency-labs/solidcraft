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
pub mod arrange;
mod ball;
pub mod blend;
pub mod brep;
pub mod bspline;
mod bvh;
pub mod catmull_rom;
pub mod cell;
mod chain;
pub mod compare;
pub mod drawing_sheet;
pub mod drawing_view;
pub mod env;
pub mod fill;
pub mod fill_general;
pub mod fill_iso;
pub mod fill_matched;
pub mod footprint;
mod grid;
pub mod hlr;
pub mod inertia;
pub mod linkage;
mod mesh;
pub mod mesh_distance;
pub mod network;
pub mod paper;
mod plane;
mod profile;
pub mod rest;
pub mod section;
pub mod simplify;
pub mod spline_space;
pub mod srgb;
pub mod stderr;
pub mod tess;
pub mod title_block;
pub mod tspline;
mod unit_vec3;
mod vec;

pub use aabb2::Aabb2;
pub use arcfit::{Loop2ArcFit, MAX_FIT_PIECES, Region2ArcFit, Seg2ArcFit};
pub use ball::Ball;
pub use blend::hermite_bezier;
pub use bvh::{Bvh, MeshIndex};
pub use chain::{Chain, Closed, Ends, Open, Path, Planar, Ring};
pub use grid::Grid;
pub use mesh::{Aabb3, BoxedTriangle, InsideIndex, MassProperties, Mesh, MeshMeasure, find_close_pair};
pub use plane::PlaneGeom;
pub use profile::{Loop2Geom, MAX_DERIVATIVE_ORDER, Region2Geom, Seg2Geom, Similarity2, bezier, regions_apart};
pub use unit_vec3::UnitVec3;
pub use vec::{Vec2Geom, Vec3Geom};

/// The geometry traits of the record's plain data ([`solidcore_record::Vec3`],
/// [`solidcore_record::Plane`], the profile types), in scope with `use solidcraft_geom::prelude::*`.
pub mod prelude {
    pub use crate::{
        Loop2ArcFit as _, Loop2Geom as _, PlaneGeom as _, Region2ArcFit as _, Region2Geom as _, Seg2ArcFit as _, Seg2Geom as _, Vec2Geom as _,
        Vec3Geom as _,
    };
}

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
