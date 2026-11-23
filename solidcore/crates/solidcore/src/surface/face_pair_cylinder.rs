//! Facing cylinders: whether two cylindrical faces look at each other across material (the two
//! sides of a tube's wall, or of a rounded bend), how thick the wall is, and the cylinder halfway
//! between them. The round counterpart of [`face_pair`](crate::surface::face_pair).
//!
//! A face's cylinder is given by a point on its axis, the axis direction, the radius, and which
//! way the face's material lies: toward the axis (the outside of a tube or a boss) or away from
//! it (a bore, the inside of a tube). Two cylinders face each other across material when they
//! share one axis (parallel within the kernel's angular tolerance, and no farther apart than its
//! zone tolerance), the outer one has its material toward the axis and the inner one away from it,
//! so that the material lies between them, and their radii differ by more than the zone
//! tolerance. A pair whose material lies the same way on both (two outsides, two bores) bounds a
//! solid or air, not a wall.
//!
//! Not upstream: no counterpart in SMLib. Deviations: none. No recursion.

use crate::error::{Result, SolidError};
use crate::geom::vector3d::{Point3, Vec3};
use crate::kernel::Kernel;

const OP: &str = "surface::face_pair_cylinder::facing_cylinders";

/// A cylindrical face's cylinder.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CylinderFace {
    /// A point on the axis.
    pub axis_point: Point3,
    /// The axis direction (any nonzero length).
    pub axis: Vec3,
    /// The radius (positive).
    pub radius: f64,
    /// Whether the face's material lies toward the axis (the outside of a tube) and not away
    /// from it (a bore).
    pub material_toward_axis: bool,
}

/// Two cylinders that face each other across material.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FacingCylinders {
    /// The wall's thickness: the outer radius less the inner. Always greater than the kernel's
    /// zone tolerance.
    pub thickness: f64,
    /// The radius of the cylinder halfway between them.
    pub mid_radius: f64,
    /// A point on the shared axis (the first cylinder's).
    pub axis_point: Point3,
    /// The shared axis's unit direction (the first cylinder's).
    pub axis: Vec3,
}

fn finite(v: Vec3) -> bool {
    v.x.is_finite() && v.y.is_finite() && v.z.is_finite()
}

/// Whether `a` and `b` face each other across material, and if so across how much. `Ok(None)` is
/// an answer, not a failure: the axes differ, or the material lies the same way on both, or the
/// radii are within the zone tolerance of each other.
///
/// # Errors
/// [`SolidError::InvalidInput`] for a point, axis or radius that is not finite, a radius that is
/// not positive, or an axis of zero length.
pub fn facing_cylinders(
    kernel: &Kernel,
    a: &CylinderFace,
    b: &CylinderFace,
) -> Result<Option<FacingCylinders>> {
    let bad = |reason| SolidError::InvalidInput {
        operation: OP,
        reason,
    };
    for c in [a, b] {
        if !(finite(c.axis_point) && finite(c.axis) && c.radius.is_finite()) {
            return Err(bad("a cylinder's point, axis and radius must be finite"));
        }
        if c.radius <= 0.0 {
            return Err(bad("a cylinder's radius must be positive"));
        }
    }
    let (ua, ub) = (a.axis.unitize()?, b.axis.unitize()?);
    let tol = kernel.tolerances;
    // One axis: parallel within the angular tolerance, and the second axis's point on the first.
    if ua.cross(ub).length() > tol.ang_tol_rad().sin() {
        return Ok(None);
    }
    if (b.axis_point - a.axis_point).cross(ua).length() > tol.zone_tol_3d {
        return Ok(None);
    }
    // The material lies between them when the outer has it toward the axis and the inner away.
    let (outer, inner) = match (a.material_toward_axis, b.material_toward_axis) {
        (true, false) => (a, b),
        (false, true) => (b, a),
        _ => return Ok(None),
    };
    let thickness = outer.radius - inner.radius;
    if thickness <= tol.zone_tol_3d {
        return Ok(None);
    }
    Ok(Some(FacingCylinders {
        thickness,
        mid_radius: inner.radius + (thickness * 0.5),
        axis_point: a.axis_point,
        axis: ua,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::KernelOptions;

    fn kernel() -> Kernel {
        Kernel::new(&KernelOptions::default())
    }

    fn cylinder(point: [f64; 3], axis: [f64; 3], radius: f64, toward: bool) -> CylinderFace {
        CylinderFace {
            axis_point: Vec3::new(point[0], point[1], point[2]),
            axis: Vec3::new(axis[0], axis[1], axis[2]),
            radius,
            material_toward_axis: toward,
        }
    }

    #[test]
    fn a_tubes_two_faces_are_its_wall_apart_with_the_mid_cylinder_between() {
        // A tube of outer radius 10 and inner radius 8 about the z axis: wall 2, mid radius 9.
        let outer = cylinder([0.0, 0.0, 0.0], [0.0, 0.0, 1.0], 10.0, true);
        let bore = cylinder([0.0, 0.0, 50.0], [0.0, 0.0, -3.0], 8.0, false);
        let pair = facing_cylinders(&kernel(), &outer, &bore).unwrap().unwrap();
        assert!((pair.thickness - 2.0).abs() < 1e-12, "{pair:?}");
        assert!((pair.mid_radius - 9.0).abs() < 1e-12, "{pair:?}");
        // The other way round: the same wall and mid radius.
        let back = facing_cylinders(&kernel(), &bore, &outer).unwrap().unwrap();
        assert!((back.thickness - 2.0).abs() < 1e-12);
        assert!((back.mid_radius - 9.0).abs() < 1e-12);
    }

    #[test]
    fn a_rounded_bend_pairs_its_inside_and_outside_radii() {
        // A 2 mm sheet bent on an inside radius of 3 about an axis along x through (0, 5, 5):
        // outside radius 5, mid radius 4.
        let outside = cylinder([0.0, 5.0, 5.0], [1.0, 0.0, 0.0], 5.0, true);
        let inside = cylinder([7.0, 5.0, 5.0], [1.0, 0.0, 0.0], 3.0, false);
        let pair = facing_cylinders(&kernel(), &outside, &inside)
            .unwrap()
            .unwrap();
        assert!(
            (pair.thickness - 2.0).abs() < 1e-12 && (pair.mid_radius - 4.0).abs() < 1e-12,
            "{pair:?}"
        );
    }

    #[test]
    fn cylinders_that_are_not_a_wall_do_not_pair() {
        let k = kernel();
        let outer = cylinder([0.0, 0.0, 0.0], [0.0, 0.0, 1.0], 10.0, true);
        // Another axis: offset, or tilted past the tolerance, or crossing.
        let offset = cylinder([0.5, 0.0, 0.0], [0.0, 0.0, 1.0], 8.0, false);
        assert_eq!(facing_cylinders(&k, &outer, &offset).unwrap(), None);
        let tilted = cylinder([0.0, 0.0, 0.0], [0.02, 0.0, 1.0], 8.0, false);
        assert_eq!(facing_cylinders(&k, &outer, &tilted).unwrap(), None);
        let across = cylinder([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], 8.0, false);
        assert_eq!(facing_cylinders(&k, &outer, &across).unwrap(), None);
        // The material lies the same way on both: two outsides, two bores.
        let outside_too = cylinder([0.0, 0.0, 0.0], [0.0, 0.0, 1.0], 8.0, true);
        assert_eq!(facing_cylinders(&k, &outer, &outside_too).unwrap(), None);
        let bore = cylinder([0.0, 0.0, 0.0], [0.0, 0.0, 1.0], 12.0, false);
        let bore_too = cylinder([0.0, 0.0, 0.0], [0.0, 0.0, 1.0], 8.0, false);
        assert_eq!(facing_cylinders(&k, &bore, &bore_too).unwrap(), None);
        // A bore wider than the outside: air between the faces, no wall.
        assert_eq!(facing_cylinders(&k, &outer, &bore).unwrap(), None);
        // The same radius: no wall.
        let same = cylinder([0.0, 0.0, 0.0], [0.0, 0.0, 1.0], 10.0, false);
        assert_eq!(facing_cylinders(&k, &outer, &same).unwrap(), None);
    }

    #[test]
    fn hostile_input_is_refused_not_answered() {
        let k = kernel();
        let good = cylinder([0.0, 0.0, 0.0], [0.0, 0.0, 1.0], 10.0, true);
        for bad in [
            cylinder([f64::NAN, 0.0, 0.0], [0.0, 0.0, 1.0], 8.0, false),
            cylinder([0.0, 0.0, 0.0], [0.0, f64::INFINITY, 1.0], 8.0, false),
            cylinder([0.0, 0.0, 0.0], [0.0, 0.0, 0.0], 8.0, false),
            cylinder([0.0, 0.0, 0.0], [0.0, 0.0, 1.0], 0.0, false),
            cylinder([0.0, 0.0, 0.0], [0.0, 0.0, 1.0], -3.0, false),
            cylinder([0.0, 0.0, 0.0], [0.0, 0.0, 1.0], f64::NAN, false),
        ] {
            assert!(facing_cylinders(&k, &good, &bad).is_err(), "{bad:?}");
            assert!(facing_cylinders(&k, &bad, &good).is_err(), "{bad:?}");
        }
    }
}
