//! Facing planes: whether two planar faces look at each other across material, how far apart
//! they are, and the plane halfway between them. The geometry under a mid-surface of a thin
//! wall (the face pairing of a wall's two sides).
//!
//! A face's plane is given as a point on it and its outward unit normal, as the face's side of
//! its shell has it. Two planes face each other across material when their outward normals run
//! opposite ways (within the kernel's angular tolerance, [`Tolerances::ang_tol_rad`]) and each
//! lies behind the other: the material is between them, so the outward normals point away from
//! each other. Planes whose outward normals point toward each other bound air, not a wall, and
//! two planes at the same place bound nothing between them: neither pairs.
//!
//! The thickness is measured along the first plane's normal from its point to where that line
//! meets the second plane (so any point of the second plane gives it), and the plane halfway
//! is parallel to it. That is exact when the planes are parallel and stands within the angular
//! tolerance's error otherwise; a pair tilted past the tolerance (a drafted wall) is not paired
//! here, it needs the bisector construction, which is not built.
//!
//! Not upstream: no counterpart in SMLib. Deviations: none. No recursion.
//!
//! [`Tolerances::ang_tol_rad`]: crate::kernel::Tolerances::ang_tol_rad

use crate::error::{Result, SolidError};
use crate::geom::vector3d::{Point3, Vec3};
use crate::kernel::Kernel;

const OP: &str = "surface::face_pair::facing_planes";

/// A planar face's plane: a point on it and the face's outward normal (any nonzero length).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OutwardPlane {
    /// A point on the plane.
    pub point: Point3,
    /// The face's outward normal, as its shell has the face's side.
    pub outward: Vec3,
}

/// Two planes that face each other across material.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FacingPlanes {
    /// The distance between the planes, along the first plane's normal: the wall's thickness.
    /// Always greater than the kernel's zone tolerance.
    pub thickness: f64,
    /// A point on the plane halfway between them.
    pub mid_point: Point3,
    /// The unit normal of the first plane: the halfway plane's side toward the first plane.
    pub toward_first: Vec3,
}

fn finite(v: Vec3) -> bool {
    v.x.is_finite() && v.y.is_finite() && v.z.is_finite()
}

/// Whether `a` and `b` face each other across material, and if so across how much.
///
/// `Ok(None)` is an answer, not a failure: the planes are not opposite within the angular
/// tolerance, or bound air, or are no farther apart than the zone tolerance.
///
/// # Errors
/// [`SolidError::InvalidInput`] for a point or normal that is not finite, or a normal of zero
/// length.
pub fn facing_planes(
    kernel: &Kernel,
    a: &OutwardPlane,
    b: &OutwardPlane,
) -> Result<Option<FacingPlanes>> {
    if !(finite(a.point) && finite(b.point) && finite(a.outward) && finite(b.outward)) {
        return Err(SolidError::InvalidInput {
            operation: OP,
            reason: "a plane's point and normal must be finite",
        });
    }
    let (ua, ub) = (a.outward.unitize()?, b.outward.unitize()?);
    // Opposite normals: pointing apart, and the sine of the angle between them within tolerance.
    let tol = kernel.tolerances;
    if ua.dot(ub) >= 0.0 || ua.cross(ub).length() > tol.ang_tol_rad().sin() {
        return Ok(None);
    }
    // How far behind `a.point`, along `ua`, the normal line through it meets `b`'s plane:
    // `(a.point - t ua - b.point) . ub = 0` gives `t = (a.point - b.point) . ub / (ua . ub)`,
    // whichever point represents `b`. Positive when `b` lies behind `a`: the material is between
    // them. `ua . ub` is below `-cos(ang_tol)` here, so the division is safe.
    let thickness = a.point.distance_along(b.point, ub) / ua.dot(ub);
    if thickness <= tol.zone_tol_3d {
        return Ok(None);
    }
    Ok(Some(FacingPlanes {
        thickness,
        mid_point: a.point - (ua * (thickness * 0.5)),
        toward_first: ua,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::KernelOptions;

    fn kernel() -> Kernel {
        Kernel::new(&KernelOptions::default())
    }

    fn plane(p: [f64; 3], n: [f64; 3]) -> OutwardPlane {
        OutwardPlane {
            point: Vec3::new(p[0], p[1], p[2]),
            outward: Vec3::new(n[0], n[1], n[2]),
        }
    }

    #[test]
    fn a_plates_two_faces_are_its_thickness_apart_with_the_mid_plane_between() {
        // A plate 2 thick: top at z = 2 facing up, bottom at z = 0 facing down. The points need
        // not be above one another.
        let top = plane([0.0, 0.0, 2.0], [0.0, 0.0, 1.0]);
        let bottom = plane([5.0, -3.0, 0.0], [0.0, 0.0, -1.0]);
        let pair = facing_planes(&kernel(), &top, &bottom).unwrap().unwrap();
        assert!((pair.thickness - 2.0).abs() < 1e-12, "{pair:?}");
        assert!((pair.mid_point.z - 1.0).abs() < 1e-12, "{pair:?}");
        assert!((pair.toward_first.z - 1.0).abs() < 1e-12);
        // The other way round: the same wall, the mid plane at the same place.
        let back = facing_planes(&kernel(), &bottom, &top).unwrap().unwrap();
        assert!((back.thickness - 2.0).abs() < 1e-12);
        assert!((back.mid_point.z - 1.0).abs() < 1e-12);
        assert!((back.toward_first.z + 1.0).abs() < 1e-12);
    }

    #[test]
    fn the_answer_does_not_depend_on_the_normals_length_or_the_frame() {
        // A wall 3 thick across the diagonal (1, 1, 0)/sqrt 2, normals not unit.
        let s = 0.5_f64.sqrt();
        let a = plane([3.0 * s, 3.0 * s, 0.0], [10.0 * s, 10.0 * s, 0.0]);
        let b = plane([0.0, 0.0, 7.0], [-s, -s, 0.0]);
        let pair = facing_planes(&kernel(), &a, &b).unwrap().unwrap();
        assert!((pair.thickness - 3.0).abs() < 1e-12, "{pair:?}");
        let mid_distance = pair.mid_point.dot(Vec3::new(s, s, 0.0));
        assert!((mid_distance - 1.5).abs() < 1e-12, "{pair:?}");
    }

    #[test]
    fn planes_that_bound_air_or_nothing_do_not_pair() {
        let k = kernel();
        // Normals pointing toward each other: a gap, not a wall.
        let low = plane([0.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
        let high = plane([0.0, 0.0, 2.0], [0.0, 0.0, -1.0]);
        assert_eq!(facing_planes(&k, &low, &high).unwrap(), None);
        // Opposite and at the same place: nothing between them.
        let same_a = plane([0.0, 0.0, 1.0], [0.0, 0.0, 1.0]);
        let same_b = plane([4.0, 4.0, 1.0], [0.0, 0.0, -1.0]);
        assert_eq!(facing_planes(&k, &same_a, &same_b).unwrap(), None);
        // Just past the zone tolerance apart pairs; at it does not.
        let zone = k.tolerances.zone_tol_3d;
        let thin = plane([0.0, 0.0, 2.0 * zone], [0.0, 0.0, 1.0]);
        let floor = plane([0.0, 0.0, 0.0], [0.0, 0.0, -1.0]);
        assert!(facing_planes(&k, &thin, &floor).unwrap().is_some());
        let at_zone = plane([0.0, 0.0, zone], [0.0, 0.0, 1.0]);
        assert_eq!(facing_planes(&k, &at_zone, &floor).unwrap(), None);
    }

    #[test]
    fn a_tilt_within_the_angular_tolerance_pairs_and_one_past_it_does_not() {
        let k = kernel();
        let top = plane([0.0, 0.0, 2.0], [0.0, 0.0, 1.0]);
        let tilted = |deg: f64| {
            let t = deg.to_radians();
            plane([0.0, 0.0, 0.0], [t.sin(), 0.0, -t.cos()])
        };
        // The tolerance is 0.5 degrees.
        assert!(facing_planes(&k, &top, &tilted(0.4)).unwrap().is_some());
        assert_eq!(facing_planes(&k, &top, &tilted(0.6)).unwrap(), None);
        // Along the same normals (not opposite) never pairs.
        assert_eq!(
            facing_planes(&k, &top, &plane([0.0, 0.0, 0.0], [0.0, 0.0, 1.0])).unwrap(),
            None
        );
        // Perpendicular walls (a rim and a face) never pair.
        assert_eq!(
            facing_planes(&k, &top, &plane([0.0, 0.0, 0.0], [1.0, 0.0, 0.0])).unwrap(),
            None
        );
    }

    #[test]
    fn a_tilted_second_plane_gives_one_thickness_whichever_point_represents_it() {
        // `a` is z = 2 facing up; `b` passes through the origin with normal (sin t, 0, -cos t),
        // t = 0.4 degrees (within the tolerance). The normal line through (0, 0, 2) is the z axis,
        // which meets `b` at the origin: thickness 2, mid point (0, 0, 1). The point
        // (1000, 0, 1000 tan t) is on `b` too, (1000 sin t - 1000 tan t cos t = 0), and must give
        // the same answer.
        let k = kernel();
        let t = 0.4_f64.to_radians();
        let top = plane([0.0, 0.0, 2.0], [0.0, 0.0, 1.0]);
        let n = [t.sin(), 0.0, -t.cos()];
        for p in [[0.0, 0.0, 0.0], [1000.0, 0.0, 1000.0 * t.tan()]] {
            let pair = facing_planes(&k, &top, &plane(p, n)).unwrap().unwrap();
            assert!((pair.thickness - 2.0).abs() < 1e-9, "{p:?}: {pair:?}");
            assert!(
                (pair.mid_point - Vec3::new(0.0, 0.0, 1.0)).length() < 1e-9,
                "{pair:?}"
            );
        }
    }

    #[test]
    fn hostile_input_is_refused_not_answered() {
        let k = kernel();
        let good = plane([0.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
        for bad in [
            plane([f64::NAN, 0.0, 0.0], [0.0, 0.0, -1.0]),
            plane([0.0, f64::INFINITY, 0.0], [0.0, 0.0, -1.0]),
            plane([0.0, 0.0, 0.0], [0.0, f64::NAN, -1.0]),
            plane([0.0, 0.0, 0.0], [0.0, 0.0, 0.0]),
        ] {
            assert!(facing_planes(&k, &good, &bad).is_err(), "{bad:?}");
            assert!(facing_planes(&k, &bad, &good).is_err(), "{bad:?}");
        }
    }
}
