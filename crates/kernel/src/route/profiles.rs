//! The router's bodies from profiles ([`solidcraft_kernel_api::profiles::Profiles`]), as
//! `solidcraft_kernel::profiles`: a profile of curves in space revolved or extruded, a sheet of
//! planar faces, a ball, and the pieces a record curve gives a profile.

use solidcore_record::brep::Curve;
use solidcraft_kernel_api::OpKind;
use solidcraft_kernel_api::affine::Matrix;
use solidcraft_kernel_api::profiles::Profiles;

pub use solidcraft_kernel_api::profiles::{ProfileCurve, ProfileRefused};

use super::{Which, which};
use crate::backend::Solidcore;
use crate::body::Body;

/// The kernel that makes bodies from profiles.
fn kernel() -> Solidcore {
    match which(OpKind::Profiles) {
        Which::Solidcore => Solidcore,
    }
}

/// The largest coordinate a profile's body may reach (mm).
pub const MAX_COORD: f64 = <Solidcore as Profiles>::MAX_COORD;

/// The kernel's zone tolerance: how near two points are the same point (mm).
///
/// # Errors
///
/// The kernel's refusal.
pub fn zone_tolerance() -> Result<f64, ProfileRefused> {
    kernel().zone_tolerance()
}

/// The closed `profile` turned `angle` (radians) about the line through `origin` along `axis`;
/// `cap` closes the ends of a turn short of a whole one. With `place`, the body is then moved by
/// that affine map (rows the images of x, y and z, then the translation), by the kernel's own move
/// of the body it made.
///
/// # Errors
///
/// The kernel's refusal: a profile that is not closed or not planar, an axis of no length, a
/// turn out of range, numbers that are not finite.
pub fn revolve_curves(
    profile: &[ProfileCurve],
    origin: [f64; 3],
    axis: [f64; 3],
    angle: f64,
    cap: bool,
    place: Option<&Matrix>,
) -> Result<Body, ProfileRefused> {
    kernel().revolve_curves(profile, origin, axis, angle, cap, place)
}

/// The closed `profile` moved `distance` along `direction`, its walls straight; `cap` closes
/// both ends.
///
/// # Errors
///
/// The kernel's refusal: a profile that is not closed or not planar, a distance out of range, a
/// direction of no length.
pub fn extrude_curves(profile: &[ProfileCurve], direction: [f64; 3], distance: f64, cap: bool) -> Result<Body, ProfileRefused> {
    kernel().extrude_curves(profile, direction, distance, cap)
}

/// A sheet of planar faces bounded by the closed `loops`.
///
/// # Errors
///
/// The kernel's refusal of the curves, or nothing made.
pub fn planar_faces(loops: &[ProfileCurve]) -> Result<Body, ProfileRefused> {
    kernel().planar_faces(loops)
}

/// A ball of `radius` about `center`, moved by `place` as [`revolve_curves`] moves its body.
///
/// # Errors
///
/// The kernel's refusal: a centre that is not finite, a radius that is not positive, a map that
/// is not a regular one.
pub fn ball(center: [f64; 3], radius: f64, place: Option<&Matrix>) -> Result<Body, ProfileRefused> {
    kernel().ball(center, radius, place)
}

/// Record curve `curve` over `interval`, as the profile curves that are it.
///
/// # Errors
///
/// The kernel's refusal: an interval that is not finite and increasing, a curve no profile
/// takes (an intersection curve).
pub fn curve_pieces(curve: &Curve, interval: [f64; 2]) -> Result<Vec<ProfileCurve>, ProfileRefused> {
    kernel().curve_pieces(curve, interval)
}
