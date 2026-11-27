//! Bodies from profiles of curves in space ([`Profiles`]): a profile revolved or extruded, a sheet
//! of planar faces, a ball, and the pieces a record curve gives a profile. What the weld bodies
//! (`crates/doc/src/weld_spot.rs`, `weld_seam.rs`) are built from.

use solidcore_record::brep::Curve;

use crate::Backend;
use crate::affine::Matrix;

/// A curve of a profile, in space.
#[derive(Clone, Debug, PartialEq)]
pub enum ProfileCurve {
    /// From `start` to `end`.
    Line {
        /// Its start.
        start: [f64; 3],
        /// Its end.
        end: [f64; 3],
    },
    /// Counter-clockwise about the unit `axis` from angle `start` to `end` (radians,
    /// 0 < end - start <= 2 pi), angle 0 along the unit `reference` (square to `axis`).
    Arc {
        /// The centre.
        center: [f64; 3],
        /// The axis.
        axis: [f64; 3],
        /// The direction of angle 0.
        reference: [f64; 3],
        /// The radius.
        radius: f64,
        /// The start angle, radians.
        start: f64,
        /// The end angle, radians.
        end: f64,
    },
    /// Control points (x, y, z, weight > 0) and the clamped knot vector (points + degree + 1
    /// knots).
    Nurbs {
        /// The degree.
        degree: usize,
        /// The control points and weights.
        points: Vec<[f64; 4]>,
        /// The knots.
        knots: Vec<f64>,
    },
}

/// A profile call the kernel refused, in the kernel's words.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct ProfileRefused(pub String);

/// Bodies from profiles of one kernel ([`crate::OpKind::Profiles`]).
pub trait Profiles: Backend {
    /// The largest coordinate a profile's body may reach (mm).
    const MAX_COORD: f64;

    /// The kernel's zone tolerance: how near two points are the same point (mm).
    ///
    /// # Errors
    ///
    /// The kernel's refusal.
    fn zone_tolerance(&self) -> Result<f64, ProfileRefused>;
    /// The closed `profile` turned `angle` (radians) about the line through `origin` along
    /// `axis`; `cap` closes the ends of a turn short of a whole one. With `place`, the body is
    /// then moved by that affine map (rows the images of x, y and z, then the translation), by
    /// the kernel's own move of the body it made.
    ///
    /// # Errors
    ///
    /// The kernel's refusal: a profile that is not closed or not planar, an axis of no length,
    /// a turn out of range, numbers that are not finite.
    fn revolve_curves(
        &self,
        profile: &[ProfileCurve],
        origin: [f64; 3],
        axis: [f64; 3],
        angle: f64,
        cap: bool,
        place: Option<&Matrix>,
    ) -> Result<Self::Form, ProfileRefused>;
    /// The closed `profile` moved `distance` along `direction`, its walls straight; `cap`
    /// closes both ends.
    ///
    /// # Errors
    ///
    /// The kernel's refusal: a profile that is not closed or not planar, a distance out of
    /// range, a direction of no length.
    fn extrude_curves(&self, profile: &[ProfileCurve], direction: [f64; 3], distance: f64, cap: bool) -> Result<Self::Form, ProfileRefused>;
    /// A sheet of planar faces bounded by the closed `loops`.
    ///
    /// # Errors
    ///
    /// The kernel's refusal of the curves, or nothing made.
    fn planar_faces(&self, loops: &[ProfileCurve]) -> Result<Self::Form, ProfileRefused>;
    /// A ball of `radius` about `center`, moved by `place` as [`Profiles::revolve_curves`]
    /// moves its body.
    ///
    /// # Errors
    ///
    /// The kernel's refusal: a centre that is not finite, a radius that is not positive, a map
    /// that is not a regular one.
    fn ball(&self, center: [f64; 3], radius: f64, place: Option<&Matrix>) -> Result<Self::Form, ProfileRefused>;
    /// Record curve `curve` over `interval`, as the profile curves that are it.
    ///
    /// # Errors
    ///
    /// The kernel's refusal: an interval that is not finite and increasing, a curve no profile
    /// takes (an intersection curve).
    fn curve_pieces(&self, curve: &Curve, interval: [f64; 2]) -> Result<Vec<ProfileCurve>, ProfileRefused>;
}
