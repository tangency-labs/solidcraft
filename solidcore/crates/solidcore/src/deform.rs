//! Space deformations of B-rep bodies and faces (surface morphing).
//!
//! A deformation is defined by a continuous 3D space map [`SpaceMap`] that sends any point
//! `P` in 3D space to a deformed point `P' = Φ(P)`. [`brep::deform_brep`] applies any
//! [`SpaceMap`] to a whole [`Brep`] or a selected subset of its faces by mapping vertices,
//! refitting face surfaces over their active parameter domains so existing 2D trim curves
//! stay parameterized in the same space, refitting edge curves to match the mapped vertices,
//! and rebuilding UV trim curves and region topology.
//!
//! Currently implemented space maps:
//! - [`surface_map::SurfaceToSurfaceMap`]: morphs geometry relative to a base surface onto
//!   a target surface.

pub mod brep;
pub mod surface_map;

use crate::error::Result;
use crate::kernel::Kernel;
use crate::math::Point3;

pub use brep::{deform_brep, deform_brep_by_surface, DeformOptions};
pub use surface_map::{MapMode, ProjectionMode, SurfaceToSurfaceMap, SurfaceToSurfaceOptions};

/// A deterministic 3D space deformation `Φ: R³ -> R³`.
pub trait SpaceMap {
    /// Map a 3D point `p` to its deformed position.
    ///
    /// # Errors
    ///
    /// Returns [`crate::SolidError`] if evaluation or projection fails or produces a
    /// non-finite coordinate.
    fn map_point(&self, k: &Kernel, p: Point3) -> Result<Point3>;
}
