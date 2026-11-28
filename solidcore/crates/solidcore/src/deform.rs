//! Space deformations of B-rep bodies and faces (surface morphing).
//!
//! A deformation is defined by a continuous 3D space map [`SpaceMap`] that sends any point
//! `P` in 3D space to a deformed point `P' = Φ(P)`. [`brep::deform_brep`] applies any
//! [`SpaceMap`] to a whole [`crate::topology::Brep`] or a selected subset of its faces by
//! mapping vertices, refitting face surfaces over their active parameter domains so existing
//! 2D trim curves stay parameterized in the same space, refitting edge curves to match the
//! mapped vertices, certifying every fitted surface cell, edge span, shared boundary edge,
//! and UV trim curve with rounded interval enclosures, and rebuilding region topology.
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
pub use surface_map::{
    is_valid_direction, MapMode, ProjectionMode, SurfaceToSurfaceMap, SurfaceToSurfaceOptions,
};

/// Closed 3D interval box `[x, y, z]`, each coordinate stored as `(lo, hi)`.
pub type Box3 = [(f64, f64); 3];

/// Closed 3×3 Jacobian interval enclosure `J[row][col]` (`∂Φ_row / ∂x_col`).
pub type Jacobian3 = [[(f64, f64); 3]; 3];

/// Centred first-order enclosure of `Φ` over a 3D box `X` about its midpoint `m`:
/// for every `P ∈ X`, `Φ(P) - Φ(m) ∈ J · (P - m) + slack`.
pub type JetEnclosure = (Jacobian3, Box3);

/// A deterministic 3D space deformation `Φ: R³ -> R³`.
pub trait SpaceMap {
    /// Map a 3D point `p` to its deformed position.
    ///
    /// # Errors
    ///
    /// Returns [`crate::SolidError`] if evaluation or projection fails or produces a
    /// non-finite coordinate.
    fn map_point(&self, k: &Kernel, p: Point3) -> Result<Point3>;

    /// Certified outward-rounded enclosure of `Φ(P)` over every `P` in `p_box`.
    ///
    /// Returns `Ok(None)` when this map cannot certify an enclosure over `p_box` (the
    /// caller refines `p_box` or refuses the operation rather than sampling).
    ///
    /// # Errors
    ///
    /// Propagates kernel cancellation and underlying geometry evaluation errors.
    fn enclose_box(&self, k: &Kernel, p_box: Box3) -> Result<Option<Box3>> {
        let _ = (k, p_box);
        Ok(None)
    }

    /// Certified outward-rounded first-order enclosure `(J, slack)` of `Φ` over `p_box`
    /// about its midpoint `m = 0.5 * (lo + hi)`, such that for all `P ∈ p_box`,
    /// `Φ(P) - Φ(m) ∈ J · (P - m) + slack`.
    ///
    /// Returns `Ok(None)` when the map cannot establish a smooth/piecewise-smooth
    /// first-order enclosure on `p_box`.
    ///
    /// # Errors
    ///
    /// Propagates kernel cancellation and underlying geometry evaluation errors.
    fn enclose_jet(&self, k: &Kernel, p_box: Box3) -> Result<Option<JetEnclosure>> {
        let _ = (k, p_box);
        Ok(None)
    }
}
