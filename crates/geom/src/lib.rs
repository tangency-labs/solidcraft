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
pub mod drawing_arrange;
pub mod drawing_break;
pub mod drawing_detail;
pub mod drawing_sheet;
pub mod drawing_view;
pub mod env;
pub mod fill;
pub mod fill_general;
pub mod fill_iso;
pub mod fill_matched;
mod grid;
pub mod ids;
pub mod inertia;
pub mod linkage;
mod mesh;
pub mod mesh_distance;
pub mod paper;
pub mod pixel_grid;
mod plane;
mod profile;
pub mod revision;
pub mod section;
pub mod simplify;
pub mod spline_space;
pub mod stderr;
pub mod table;
/// The plan evaluator and the display plan, solidcore-tess's (RFC 0003, items 1.1 and 1.8;
/// `solidcore_tess::eval`), under their old path.
pub mod tess {
    pub use solidcore_tess::eval::{MAX_EVAL_KNOTS, MAX_EVAL_POINTS, MAX_SPLINE_DEGREE, basis, basis_ders, span_of, uv};
    pub use solidcore_tess::{EvalCurve, EvalSurface, FacePlan, TessPlan};

    use solidcore_record::Vec3;

    /// A vector of SolidCraft's as the kernel's.
    pub fn to_kernel(v: Vec3) -> solidcore_tess::eval::Vec3 {
        solidcore_tess::eval::Vec3::new(v.x, v.y, v.z)
    }

    /// A vector of the kernel's as SolidCraft's.
    pub fn from_kernel(v: solidcore_tess::eval::Vec3) -> Vec3 {
        Vec3::new(v.x, v.y, v.z)
    }

    /// A point and two derivatives of the kernel's as SolidCraft's.
    pub fn from_kernel3((a, b, c): (solidcore_tess::eval::Vec3, solidcore_tess::eval::Vec3, solidcore_tess::eval::Vec3)) -> (Vec3, Vec3, Vec3) {
        (from_kernel(a), from_kernel(b), from_kernel(c))
    }

    /// The kernel's partial derivatives ([`solidcore_tess::eval::partials`]) in SolidCraft's
    /// vectors.
    pub mod partials {
        use solidcore_record::Vec3;
        pub use solidcore_tess::eval::partials::MAX_PARTIAL_ORDER;
        pub use solidcore_tess::eval::partials::orders;
        pub use solidcore_tess::eval::partials::trig_derivative;

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
        pub fn from_kernel(d: solidcore_tess::eval::partials::Partials) -> Partials {
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
pub use arcfit::Loop2ArcFit;
pub use arcfit::MAX_FIT_PIECES;
pub use arcfit::Region2ArcFit;
pub use arcfit::Seg2ArcFit;
pub use ball::Ball;
pub use blend::hermite_bezier;
pub use bvh::Bvh;
pub use bvh::MeshIndex;
pub use bvh::PointCloudIndex;
pub use chain::Chain;
pub use chain::Closed;
pub use chain::Ends;
pub use chain::Open;
pub use chain::Path;
pub use chain::Planar;
pub use chain::Ring;
pub use grid::Grid;
pub use mesh::Aabb3;
pub use mesh::BoxedTriangle;
pub use mesh::InsideIndex;
pub use mesh::MassProperties;
pub use mesh::Mesh;
pub use mesh::MeshGeom;
pub use mesh::MeshMeasure;
pub use mesh::TANGENT_CHAIN;
pub use mesh::find_close_pair;
pub use plane::PlaneGeom;
pub use profile::ArcMap;
pub use profile::Loop2Geom;
pub use profile::MAX_DERIVATIVE_ORDER;
pub use profile::Region2Geom;
pub use profile::Seg2Geom;
pub use profile::Similarity2;
pub use profile::bezier;
pub use profile::regions_apart;
pub use unit_vec3::UnitVec3;
pub use vec::Vec2Geom;
pub use vec::Vec3Geom;

/// The geometry traits of the record's plain data ([`solidcore_record::Vec3`],
/// [`solidcore_record::Plane`], the profile types), in scope with `use solidcraft_geom::prelude::*`.
pub mod prelude {
    pub use crate::Loop2ArcFit as _;
    pub use crate::Loop2Geom as _;
    pub use crate::MeshGeom as _;
    pub use crate::PlaneGeom as _;
    pub use crate::Region2ArcFit as _;
    pub use crate::Region2Geom as _;
    pub use crate::Seg2ArcFit as _;
    pub use crate::Seg2Geom as _;
    pub use crate::Vec2Geom as _;
    pub use crate::Vec3Geom as _;
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
