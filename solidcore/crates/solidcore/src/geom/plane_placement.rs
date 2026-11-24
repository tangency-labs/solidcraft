//! How one plane frame sits relative to another, and how an axis sits in a plane frame (not
//! upstream): the placement questions an importer asks when it maps another program's frames
//! onto planes it can keep editable.
//!
//! - [`normal_offset`]: whether a frame is a base frame moved along the base's normal and turned
//!   about it (no in-plane shift, no tilt), and if so by how far and how much.
//! - [`axis_in_plane`]: whether an axis lies in a plane, and the axis in the plane's own
//!   coordinates.
//!
//! A frame is its origin and unit, square `x` and `y` axes; its normal is `x × y`.

use crate::geom::vector3d::Vec3;

/// How far an origin may stand off the base's normal line, in model units, for
/// [`normal_offset`] to call the frame a normal offset of the base.
pub const SIDEWAYS_TOLERANCE: f64 = 1e-7;

/// How far below 1 the cosine between the two normals may fall for [`normal_offset`] to call
/// them the same direction (about 1.4e-5 rad).
pub const NORMAL_COSINE_TOLERANCE: f64 = 1e-10;

/// A frame that is a base frame moved along the base's normal and turned about it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NormalOffset {
    /// The signed distance of the frame's origin from the base, along the base's normal.
    pub distance: f64,
    /// The frame's `x` turned from the base's `x` about the base's normal, in radians in
    /// (-π, π], positive counter-clockwise looking down the normal.
    pub rotation: f64,
}

/// Whether the frame (`origin`, `x`, `y`) is the base frame (`base_origin`, `base_x`, `base_y`)
/// moved along the base's normal and turned about it: its origin is on the normal line through
/// the base's origin (within [`SIDEWAYS_TOLERANCE`]) and its normal faces the same way (cosine
/// within [`NORMAL_COSINE_TOLERANCE`] of 1). `None` when the frame is shifted within the base
/// plane, tilted from it or faces the other way.
#[must_use]
pub fn normal_offset(base: [Vec3; 3], frame: [Vec3; 3]) -> Option<NormalOffset> {
    let [base_origin, base_x, base_y] = base;
    let [origin, x, y] = frame;
    let n = base_x.cross(base_y);
    let delta = origin - base_origin;
    let distance = delta.dot(n);
    let sideways = delta - n * distance;
    if sideways.length_squared().sqrt() > SIDEWAYS_TOLERANCE
        || x.cross(y).dot(n) < 1.0 - NORMAL_COSINE_TOLERANCE
    {
        return None;
    }
    let rotation = n.dot(base_x.cross(x)).atan2(base_x.dot(x));
    Some(NormalOffset { distance, rotation })
}

/// An axis that lies in a plane, from [`axis_in_plane`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlanarAxis {
    /// The axis's unit direction in model space.
    pub unit: Vec3,
    /// The axis's point in the plane's coordinates.
    pub origin: [f64; 2],
    /// The axis's unit direction in the plane's coordinates.
    pub direction: [f64; 2],
}

impl PlanarAxis {
    /// Whether `v` points against the axis (a negative dot product with its direction).
    #[must_use]
    pub fn opposes(&self, v: Vec3) -> bool {
        v.dot(self.unit) < 0.0
    }
}

/// Why [`axis_in_plane`] found no [`PlanarAxis`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AxisOffPlane {
    /// The direction is under 1e-9 long, or not finite.
    NoDirection,
    /// The axis is tilted from the plane (the sine of its angle to the plane over 1e-7) or its
    /// point stands off the plane (over 1e-6 model units).
    OffPlane,
}

/// The axis through `origin` along `direction` in the plane frame's (`plane_origin`, `x`, `y`)
/// own coordinates, when it lies in the plane.
///
/// # Errors
/// [`AxisOffPlane::NoDirection`] for a direction that has no length, and
/// [`AxisOffPlane::OffPlane`] for an axis that does not lie in the plane.
pub fn axis_in_plane(
    plane: [Vec3; 3],
    origin: Vec3,
    direction: Vec3,
) -> Result<PlanarAxis, AxisOffPlane> {
    let [plane_origin, x, y] = plane;
    let length = direction.length_squared().sqrt();
    if !(length > 1e-9 && length.is_finite()) {
        return Err(AxisOffPlane::NoDirection);
    }
    let unit = direction / length;
    let n = x.cross(y);
    let from_origin = origin - plane_origin;
    if unit.dot(n).abs() > 1e-7 || from_origin.dot(n).abs() > 1e-6 {
        return Err(AxisOffPlane::OffPlane);
    }
    Ok(PlanarAxis {
        unit,
        origin: [from_origin.dot(x), from_origin.dot(y)],
        direction: [unit.dot(x), unit.dot(y)],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: f64, y: f64, z: f64) -> Vec3 {
        Vec3::new(x, y, z)
    }

    fn xy_at(z: f64) -> [Vec3; 3] {
        [v(0.0, 0.0, z), Vec3::X, Vec3::Y]
    }

    #[test]
    fn a_frame_moved_along_its_normal_is_a_normal_offset_with_no_rotation() {
        let offset = normal_offset(xy_at(0.0), xy_at(-12.5)).unwrap();
        assert_eq!(
            offset,
            NormalOffset {
                distance: -12.5,
                rotation: 0.0
            }
        );
    }

    #[test]
    fn a_frame_turned_about_the_normal_reports_its_rotation() {
        // x turned 90° counter-clockwise looking down +z: x = +y, y = -x.
        let offset =
            normal_offset(xy_at(0.0), [v(0.0, 0.0, 3.0), Vec3::Y, v(-1.0, 0.0, 0.0)]).unwrap();
        assert_eq!(offset.distance, 3.0);
        assert!(
            (offset.rotation - std::f64::consts::FRAC_PI_2).abs() < 1e-15,
            "{offset:?}"
        );
        let back =
            normal_offset(xy_at(0.0), [v(0.0, 0.0, 3.0), v(0.0, -1.0, 0.0), Vec3::X]).unwrap();
        assert!(
            (back.rotation + std::f64::consts::FRAC_PI_2).abs() < 1e-15,
            "{back:?}"
        );
    }

    #[test]
    fn a_shift_within_the_plane_a_tilt_or_a_flip_is_not_a_normal_offset() {
        let base = xy_at(0.0);
        assert_eq!(
            normal_offset(base, [v(1e-6, 0.0, 5.0), Vec3::X, Vec3::Y]),
            None
        );
        assert!(
            normal_offset(base, [v(5e-8, 0.0, 5.0), Vec3::X, Vec3::Y]).is_some(),
            "under the sideways tolerance"
        );
        let tilt = 1e-3_f64;
        assert_eq!(
            normal_offset(
                base,
                [v(0.0, 0.0, 1.0), Vec3::X, v(0.0, tilt.cos(), tilt.sin())]
            ),
            None
        );
        assert_eq!(
            normal_offset(base, [v(0.0, 0.0, 1.0), Vec3::X, v(0.0, -1.0, 0.0)]),
            None
        );
    }

    #[test]
    fn an_axis_in_the_plane_comes_back_in_plane_coordinates() {
        let plane = [v(1.0, 2.0, 3.0), Vec3::Y, Vec3::Z];
        let axis = axis_in_plane(plane, v(1.0, 5.0, 7.0), v(0.0, 0.0, 2.0)).unwrap();
        assert_eq!(axis.unit, Vec3::Z);
        assert_eq!(axis.origin, [3.0, 4.0]);
        assert_eq!(axis.direction, [0.0, 1.0]);
        assert!(
            axis.opposes(v(0.0, 0.0, -1.0)) && !axis.opposes(Vec3::Z) && !axis.opposes(Vec3::X)
        );
    }

    #[test]
    fn an_axis_off_the_plane_or_without_direction_is_refused() {
        let plane = xy_at(0.0);
        assert_eq!(
            axis_in_plane(plane, Vec3::ZERO, Vec3::ZERO),
            Err(AxisOffPlane::NoDirection)
        );
        assert_eq!(
            axis_in_plane(plane, Vec3::ZERO, v(f64::NAN, 0.0, 0.0)),
            Err(AxisOffPlane::NoDirection)
        );
        assert_eq!(
            axis_in_plane(plane, Vec3::ZERO, v(1.0, 0.0, 1e-3)),
            Err(AxisOffPlane::OffPlane)
        );
        assert_eq!(
            axis_in_plane(plane, v(0.0, 0.0, 1e-5), Vec3::X),
            Err(AxisOffPlane::OffPlane)
        );
        assert!(axis_in_plane(plane, v(0.0, 0.0, 1e-7), Vec3::X).is_ok());
    }
}
