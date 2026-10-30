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
mod grid;
pub mod hlr;
pub mod hull3;
pub mod inertia;
pub mod linkage;
mod mesh;
pub mod mesh_distance;
pub mod network;
pub mod paper;
pub mod pixel_grid;
mod plane;
mod profile;
pub mod section;
pub mod simplify;
pub mod spline_space;
pub mod stderr;
/// The plan evaluator, now the kernel's (`solidcore::eval::plan`), and the display plan, now
/// solidcore-tess's (RFC 0003, item 1.1), under their old path.
pub mod tess {
    pub use solidcore::eval::plan::*;
    pub use solidcore_tess::{FacePlan, TessPlan};

    use solidcore_record::Vec3;

    /// A vector of SolidCraft's as the kernel's.
    pub fn to_kernel(v: Vec3) -> solidcore::Vec3 {
        solidcore::Vec3::new(v.x, v.y, v.z)
    }

    /// A vector of the kernel's as SolidCraft's.
    pub fn from_kernel(v: solidcore::Vec3) -> Vec3 {
        Vec3::new(v.x, v.y, v.z)
    }

    /// A point and two derivatives of the kernel's as SolidCraft's.
    pub fn from_kernel3((a, b, c): (solidcore::Vec3, solidcore::Vec3, solidcore::Vec3)) -> (Vec3, Vec3, Vec3) {
        (from_kernel(a), from_kernel(b), from_kernel(c))
    }

    /// The kernel's partial derivatives ([`solidcore::eval::plan::partials`]) in SolidCraft's
    /// vectors.
    pub mod partials {
        pub use solidcore::eval::plan::partials::{MAX_PARTIAL_ORDER, orders, trig_derivative};
        use solidcore_record::Vec3;

        /// A surface's partial derivatives at a point: `d[i][j]` is `∂^(i+j) S / ∂u^i ∂v^j`, filled
        /// for `i + j ≤` [`MAX_PARTIAL_ORDER`] and zero past it.
        pub type Partials = [[Vec3; MAX_PARTIAL_ORDER + 1]; MAX_PARTIAL_ORDER + 1];

        /// A table of partials from `f(i, j)`; `None` when one is missing or not finite.
        pub fn tabulate(mut f: impl FnMut(usize, usize) -> Option<Vec3>) -> Option<Partials> {
            let mut d = [[Vec3::ZERO; MAX_PARTIAL_ORDER + 1]; MAX_PARTIAL_ORDER + 1];
            for (i, j) in orders() {
                let x = f(i, j).filter(|x| x.is_finite())?;
                *d.get_mut(i)?.get_mut(j)? = x;
            }
            Some(d)
        }

        /// The kernel's table in SolidCraft's vectors.
        pub fn from_kernel(d: solidcore::eval::plan::partials::Partials) -> Partials {
            d.map(|row| row.map(super::from_kernel))
        }
    }

    /// A plan evaluated into SolidCraft's [`crate::Mesh`].
    pub trait EvaluateMesh {
        /// The plan's mesh ([`TessPlan::evaluate`]), as SolidCraft's mesh.
        fn evaluate_mesh(&self) -> crate::Mesh;
    }

    impl EvaluateMesh for TessPlan {
        fn evaluate_mesh(&self) -> crate::Mesh {
            self.evaluate().0
        }
    }
}
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
pub use mesh::{Aabb3, BoxedTriangle, InsideIndex, MassProperties, Mesh, MeshGeom, MeshMeasure, TANGENT_CHAIN, find_close_pair};
pub use plane::PlaneGeom;
pub use profile::{Loop2Geom, MAX_DERIVATIVE_ORDER, Region2Geom, Seg2Geom, Similarity2, bezier, regions_apart};
pub use unit_vec3::UnitVec3;
pub use vec::{Vec2Geom, Vec3Geom};

/// The geometry traits of the record's plain data ([`solidcore_record::Vec3`],
/// [`solidcore_record::Plane`], the profile types), in scope with `use solidcraft_geom::prelude::*`.
pub mod prelude {
    pub use crate::{
        Loop2ArcFit as _, Loop2Geom as _, MeshGeom as _, PlaneGeom as _, Region2ArcFit as _, Region2Geom as _, Seg2ArcFit as _, Seg2Geom as _,
        Vec2Geom as _, Vec3Geom as _,
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
