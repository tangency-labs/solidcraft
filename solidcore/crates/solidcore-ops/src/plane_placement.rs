//! How one sketch plane sits relative to another, and how an axis sits in a sketch plane: the
//! kernel's [`solidcore::geom::plane_placement`] on records' planes. Ops converts.
//!
//! - [`normal_offset`]: whether a plane is a base plane moved along the base's normal and
//!   turned about it, and if so by how far and how much (an importer keeps such a plane as an
//!   offset of the base).
//! - [`axis_in_plane`]: whether an axis lies in a plane, and the axis in the plane's own
//!   coordinates (a revolve axis).

use solidcore::geom::plane_placement as k;
use solidcore::geom::vector3d::Vec3;
use solidcore_record::{Plane, Vec2, Vec3 as RecordVec3};

pub use solidcore::geom::plane_placement::{AxisOffPlane, NormalOffset};

fn vec(v: RecordVec3) -> Vec3 {
    Vec3::new(v.x, v.y, v.z)
}

fn frame(p: &Plane) -> [Vec3; 3] {
    [vec(p.origin), vec(p.x), vec(p.y)]
}

/// Whether `plane` is `base` moved along `base`'s normal (`x × y`) and turned about it: its
/// origin on the normal line through `base`'s origin (within 1e-7) and its normal the same way
/// (cosine within 1e-10 of 1). `None` when it is shifted within the base plane, tilted from it
/// or faces the other way. The kernel's [`k::normal_offset`].
#[must_use]
pub fn normal_offset(base: &Plane, plane: &Plane) -> Option<NormalOffset> {
    k::normal_offset(frame(base), frame(plane))
}

/// An axis that lies in a plane, from [`axis_in_plane`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlanarAxis {
    /// The axis's unit direction in model space.
    pub unit: RecordVec3,
    /// The axis's point in the plane's coordinates.
    pub origin: Vec2,
    /// The axis's unit direction in the plane's coordinates.
    pub direction: Vec2,
}

impl PlanarAxis {
    /// Whether `v` points against the axis (a negative dot product with its direction).
    #[must_use]
    pub fn opposes(&self, v: RecordVec3) -> bool {
        k::PlanarAxis {
            unit: vec(self.unit),
            origin: [0.0; 2],
            direction: [0.0; 2],
        }
        .opposes(vec(v))
    }
}

/// The axis through `origin` along `direction` in `plane`'s own coordinates, when it lies in
/// the plane: the kernel's [`k::axis_in_plane`].
///
/// # Errors
/// [`AxisOffPlane::NoDirection`] for a direction under 1e-9 long or not finite, and
/// [`AxisOffPlane::OffPlane`] for an axis tilted from the plane (over 1e-7) or standing off it
/// (over 1e-6).
pub fn axis_in_plane(
    plane: &Plane,
    origin: RecordVec3,
    direction: RecordVec3,
) -> Result<PlanarAxis, AxisOffPlane> {
    let axis = k::axis_in_plane(frame(plane), vec(origin), vec(direction))?;
    Ok(PlanarAxis {
        unit: RecordVec3::new(axis.unit.x, axis.unit.y, axis.unit.z),
        origin: Vec2::from(axis.origin),
        direction: Vec2::from(axis.direction),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: f64, y: f64, z: f64) -> RecordVec3 {
        RecordVec3::new(x, y, z)
    }

    #[test]
    fn records_planes_reach_the_kernel_and_back() {
        let base = Plane {
            origin: v(0.0, 0.0, 0.0),
            x: v(1.0, 0.0, 0.0),
            y: v(0.0, 1.0, 0.0),
        };
        let turned = Plane {
            origin: v(0.0, 0.0, 3.0),
            x: v(0.0, 1.0, 0.0),
            y: v(-1.0, 0.0, 0.0),
        };
        let offset = normal_offset(&base, &turned).unwrap();
        assert_eq!(offset.distance, 3.0);
        assert!(
            (offset.rotation - std::f64::consts::FRAC_PI_2).abs() < 1e-15,
            "{offset:?}"
        );
        assert_eq!(
            normal_offset(
                &base,
                &Plane {
                    origin: v(1.0, 0.0, 3.0),
                    ..base
                }
            ),
            None
        );

        let side = Plane {
            origin: v(1.0, 2.0, 3.0),
            x: v(0.0, 1.0, 0.0),
            y: v(0.0, 0.0, 1.0),
        };
        let axis = axis_in_plane(&side, v(1.0, 5.0, 7.0), v(0.0, 0.0, 2.0)).unwrap();
        assert_eq!(
            axis,
            PlanarAxis {
                unit: v(0.0, 0.0, 1.0),
                origin: Vec2::new(3.0, 4.0),
                direction: Vec2::new(0.0, 1.0)
            }
        );
        assert!(axis.opposes(v(0.0, 0.0, -1.0)) && !axis.opposes(v(0.0, 0.0, 1.0)));
        assert_eq!(
            axis_in_plane(&side, v(2.0, 0.0, 0.0), v(0.0, 1.0, 0.0)),
            Err(AxisOffPlane::OffPlane)
        );
        assert_eq!(
            axis_in_plane(&side, v(1.0, 0.0, 0.0), v(0.0, 0.0, 0.0)),
            Err(AxisOffPlane::NoDirection)
        );
    }
}
