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
//! Also the small readings an application needs about one such face: which side its material
//! lies on ([`material_toward_axis`]), the directions that stay on it from one of its points
//! ([`round_directions`]), and one cylinder's axial range in another's coordinates
//! ([`axial_range_in`]).
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

/// Whether a cylindrical face's material lies toward its axis, read at one `point` of the face
/// from the face's `outward` normal there: the normal points away from the axis exactly when the
/// material lies toward it (the outside of a tube), and toward the axis on a bore. The point's
/// part at right angles to the axis is the direction away from the axis; the answer is whether
/// the normal has a positive part along it.
///
/// # Errors
/// [`SolidError::InvalidInput`] for a point or vector that is not finite, an axis or normal of
/// zero length, or a point on the axis (where no direction is away from it).
pub fn material_toward_axis(
    axis_point: Point3,
    axis: Vec3,
    point: Point3,
    outward: Vec3,
) -> Result<bool> {
    let bad = |reason| SolidError::InvalidInput {
        operation: "surface::face_pair_cylinder::material_toward_axis",
        reason,
    };
    if !(finite(axis_point) && finite(axis) && finite(point) && finite(outward)) {
        return Err(bad("the axis, point and normal must be finite"));
    }
    let u = axis.unitize()?;
    if outward.length_squared() < crate::math::EFF_ZERO_SQ {
        return Err(bad("the normal must not be of zero length"));
    }
    let from = point - axis_point;
    let away = from - (u * from.dot(u));
    if away.length_squared() < crate::math::EFF_ZERO_SQ {
        return Err(bad("the point lies on the axis"));
    }
    Ok(outward.dot(away) > 0.0)
}

/// The unit directions to step along from a point of a cylindrical face to stay on the face:
/// along the axis both ways, then round it both ways, the round one being `axis × along` made
/// unit, where `along` is the direction of a curve through the point (an edge's tangent). When
/// the curve runs along the axis (`axis × along` shorter than a billionth of `along`) no
/// direction round the axis follows from it and only the two along the axis are given.
///
/// # Errors
/// [`SolidError::InvalidInput`] for a vector that is not finite, or an axis of zero length.
pub fn round_directions(axis: Vec3, along: Vec3) -> Result<Vec<Vec3>> {
    if !(finite(axis) && finite(along)) {
        return Err(SolidError::InvalidInput {
            operation: "surface::face_pair_cylinder::round_directions",
            reason: "the axis and the curve's direction must be finite",
        });
    }
    let u = axis.unitize()?;
    let round = u.cross(along);
    if round.length() <= along.length() * 1e-9 {
        return Ok(vec![u, -u]);
    }
    let round = round.unitize()?;
    Ok(vec![u, -u, round, -round])
}

/// The axial range `range` of cylinder `b` (distances from its axis point along its unit axis)
/// read in cylinder `a`'s axial coordinates (distances from `a`'s axis point along `a`'s unit
/// axis), low end first. A coordinate `x` on `b`'s axis is the point `pb + x·ub`; its coordinate
/// on `a`'s axis is `(pb + x·ub − pa)·ua = shift + x·turn`, with `shift = (pb − pa)·ua` and
/// `turn = ub·ua` (±1 for one axis run either way). The radii and the material's sides play no
/// part.
///
/// # Errors
/// [`SolidError::InvalidInput`] for an axis point, axis or range end that is not finite, or an
/// axis of zero length.
pub fn axial_range_in(a: &CylinderFace, b: &CylinderFace, range: [f64; 2]) -> Result<[f64; 2]> {
    let [lo, hi] = range;
    if !(finite(a.axis_point)
        && finite(a.axis)
        && finite(b.axis_point)
        && finite(b.axis)
        && lo.is_finite()
        && hi.is_finite())
    {
        return Err(SolidError::InvalidInput {
            operation: "surface::face_pair_cylinder::axial_range_in",
            reason: "the axis points, axes and range must be finite",
        });
    }
    let (ua, ub) = (a.axis.unitize()?, b.axis.unitize()?);
    let shift = (b.axis_point - a.axis_point).dot(ua);
    let turn = ub.dot(ua);
    let ends = [shift + (lo * turn), shift + (hi * turn)];
    Ok([ends[0].min(ends[1]), ends[0].max(ends[1])])
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

    fn v(x: f64, y: f64, z: f64) -> Vec3 {
        Vec3::new(x, y, z)
    }

    #[test]
    fn the_material_side_follows_the_normal_against_the_radial_direction() {
        // Axis z through the origin. At (10, 0, 5) the direction away from the axis is +x: an
        // outward normal +x is a tube's outside (material toward the axis), -x a bore's.
        let (o, z) = (v(0.0, 0.0, 0.0), v(0.0, 0.0, 1.0));
        assert!(material_toward_axis(o, z, v(10.0, 0.0, 5.0), v(1.0, 0.0, 0.0)).unwrap());
        assert!(!material_toward_axis(o, z, v(10.0, 0.0, 5.0), v(-1.0, 0.0, 0.0)).unwrap());
        // The same axis given by another point on it and run the other way, any length: at
        // (0, -8, 7) away from the axis is -y, so a normal -y (any length) is an outside.
        let (o2, down) = (v(0.0, 0.0, 50.0), v(0.0, 0.0, -3.0));
        assert!(material_toward_axis(o2, down, v(0.0, -8.0, 7.0), v(0.0, -2.0, 0.0)).unwrap());
        assert!(!material_toward_axis(o2, down, v(0.0, -8.0, 7.0), v(0.0, 0.5, 0.0)).unwrap());
    }

    #[test]
    fn hostile_material_side_queries_are_refused() {
        let (o, z, p, n) = (
            v(0.0, 0.0, 0.0),
            v(0.0, 0.0, 1.0),
            v(10.0, 0.0, 0.0),
            v(1.0, 0.0, 0.0),
        );
        let nan = v(f64::NAN, 0.0, 0.0);
        let zero = v(0.0, 0.0, 0.0);
        assert!(material_toward_axis(nan, z, p, n).is_err());
        assert!(material_toward_axis(o, z, p, v(0.0, f64::INFINITY, 0.0)).is_err());
        assert!(material_toward_axis(o, zero, p, n).is_err());
        assert!(material_toward_axis(o, z, p, zero).is_err());
        // A point on the axis has no direction away from it.
        assert!(material_toward_axis(o, z, v(0.0, 0.0, 9.0), n).is_err());
    }

    #[test]
    fn directions_round_the_axis_are_axis_cross_along() {
        // Axis z (length 2), an edge running along +y: z × y = -x, so the four are +z, -z, -x, +x.
        let d = round_directions(v(0.0, 0.0, 2.0), v(0.0, 5.0, 0.0)).unwrap();
        assert_eq!(
            d,
            vec![
                v(0.0, 0.0, 1.0),
                v(0.0, 0.0, -1.0),
                v(-1.0, 0.0, 0.0),
                v(1.0, 0.0, 0.0)
            ]
        );
        // An edge along the axis (a straight seam), or within a billionth of it, or no direction:
        // only the two along the axis.
        let z2 = vec![v(0.0, 0.0, 1.0), v(0.0, 0.0, -1.0)];
        assert_eq!(
            round_directions(v(0.0, 0.0, 1.0), v(0.0, 0.0, -4.0)).unwrap(),
            z2
        );
        assert_eq!(
            round_directions(v(0.0, 0.0, 1.0), v(1e-12, 0.0, 1.0)).unwrap(),
            z2
        );
        assert_eq!(
            round_directions(v(0.0, 0.0, 1.0), v(0.0, 0.0, 0.0)).unwrap(),
            z2
        );
        // Hostile: not finite, or no axis.
        assert!(round_directions(v(f64::NAN, 0.0, 1.0), v(0.0, 1.0, 0.0)).is_err());
        assert!(round_directions(v(0.0, 0.0, 1.0), v(0.0, f64::INFINITY, 0.0)).is_err());
        assert!(round_directions(v(0.0, 0.0, 0.0), v(0.0, 1.0, 0.0)).is_err());
    }

    #[test]
    fn an_axial_range_reads_in_the_other_cylinders_coordinates() {
        let a = cylinder([0.0, 0.0, 0.0], [0.0, 0.0, 1.0], 10.0, true);
        // b's axis point at z = 50 and its axis run down (length 3): its coordinate x is the
        // height 50 - x, so its range 10..30 is the heights 20..40 (low end first).
        let down = cylinder([0.0, 0.0, 50.0], [0.0, 0.0, -3.0], 8.0, false);
        assert_eq!(
            axial_range_in(&a, &down, [10.0, 30.0]).unwrap(),
            [20.0, 40.0]
        );
        // b's axis point at z = 5 (given off the line: only its height counts), the same way up:
        // 0..10 is 5..15.
        let up = cylinder([3.0, 4.0, 5.0], [0.0, 0.0, 1.0], 8.0, false);
        assert_eq!(axial_range_in(&a, &up, [0.0, 10.0]).unwrap(), [5.0, 15.0]);
        // Hostile: a range end or axis that is not finite, an axis of zero length.
        assert!(axial_range_in(&a, &up, [f64::NAN, 1.0]).is_err());
        assert!(axial_range_in(&a, &up, [0.0, f64::INFINITY]).is_err());
        let none = cylinder([0.0, 0.0, 0.0], [0.0, 0.0, 0.0], 8.0, false);
        assert!(axial_range_in(&a, &none, [0.0, 1.0]).is_err());
        assert!(axial_range_in(&none, &a, [0.0, 1.0]).is_err());
        let nan = cylinder([f64::NAN, 0.0, 0.0], [0.0, 0.0, 1.0], 8.0, false);
        assert!(axial_range_in(&nan, &a, [0.0, 1.0]).is_err());
    }
}
