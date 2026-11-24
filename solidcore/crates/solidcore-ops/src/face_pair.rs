//! Facing planes for an application: whether two planar faces look at each other across
//! material, how far apart they are, and the plane halfway between them
//! ([`solidcore::surface::face_pair::facing_planes`], at the kernel's angular and zone
//! tolerances), the same for cylinders, and a cylindrical face's material side, the directions
//! that stay on it, and one cylinder's axial range in another's coordinates. Ops converts the
//! arrays; the kernel decides.

use solidcore::surface::face_pair::{facing_planes as kernel_facing_planes, OutwardPlane as Plane};
use solidcore::surface::face_pair_cylinder::{
    axial_range_in as kernel_axial_range_in, facing_cylinders as kernel_facing_cylinders,
    material_toward_axis as kernel_material_toward_axis,
    round_directions as kernel_round_directions, CylinderFace as KernelCylinder,
};
use solidcore::Vec3;

use crate::session::run;
use crate::Result;

/// A planar face's plane: a point on it and the face's outward normal (any nonzero length).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OutwardPlane {
    /// A point on the plane.
    pub point: [f64; 3],
    /// The face's outward normal, as its shell has the face's side.
    pub outward: [f64; 3],
}

/// Two planes that face each other across material.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FacingPlanes {
    /// The distance between the planes along the first plane's normal: the wall's thickness.
    pub thickness: f64,
    /// A point on the plane halfway between them.
    pub mid_point: [f64; 3],
    /// The first plane's unit normal: the halfway plane's side toward the first plane.
    pub toward_first: [f64; 3],
}

fn plane(p: &OutwardPlane) -> Plane {
    Plane {
        point: Vec3::new(p.point[0], p.point[1], p.point[2]),
        outward: Vec3::new(p.outward[0], p.outward[1], p.outward[2]),
    }
}

/// Whether `a` and `b` face each other across material, and if so across how much. `None` is an
/// answer, not a failure: the planes are not opposite within the kernel's angular tolerance, or
/// bound air, or are no farther apart than its zone tolerance.
///
/// # Errors
/// The kernel's refusal of a point or normal that is not finite, or a normal of zero length
/// ([`crate::Error::Status`]).
pub fn facing_planes(a: &OutwardPlane, b: &OutwardPlane) -> Result<Option<FacingPlanes>> {
    run("face_pair::facing_planes", |k| {
        Ok(
            kernel_facing_planes(k, &plane(a), &plane(b))?.map(|f| FacingPlanes {
                thickness: f.thickness,
                mid_point: [f.mid_point.x, f.mid_point.y, f.mid_point.z],
                toward_first: [f.toward_first.x, f.toward_first.y, f.toward_first.z],
            }),
        )
    })
}

/// A cylindrical face's cylinder: a point on its axis, the axis, the radius, and whether the
/// face's material lies toward the axis (the outside of a tube) and not away from it (a bore).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CylinderFace {
    /// A point on the axis.
    pub axis_point: [f64; 3],
    /// The axis direction (any nonzero length).
    pub axis: [f64; 3],
    /// The radius (positive).
    pub radius: f64,
    /// Whether the material lies toward the axis.
    pub material_toward_axis: bool,
}

/// Two cylinders that face each other across material.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FacingCylinders {
    /// The wall's thickness: the outer radius less the inner.
    pub thickness: f64,
    /// The radius of the cylinder halfway between them.
    pub mid_radius: f64,
    /// A point on the shared axis.
    pub axis_point: [f64; 3],
    /// The shared axis's unit direction.
    pub axis: [f64; 3],
}

fn cylinder(c: &CylinderFace) -> KernelCylinder {
    KernelCylinder {
        axis_point: Vec3::new(c.axis_point[0], c.axis_point[1], c.axis_point[2]),
        axis: Vec3::new(c.axis[0], c.axis[1], c.axis[2]),
        radius: c.radius,
        material_toward_axis: c.material_toward_axis,
    }
}

/// Whether the cylinders `a` and `b` face each other across material (one axis, the outer's
/// material toward it and the inner's away, the radii apart by more than the kernel's zone
/// tolerance), and if so across how much. `None` is an answer, not a failure.
///
/// # Errors
/// The kernel's refusal ([`crate::Error::Status`]) of a point, axis or radius that is not finite,
/// a radius that is not positive, or an axis of zero length.
pub fn facing_cylinders(a: &CylinderFace, b: &CylinderFace) -> Result<Option<FacingCylinders>> {
    run("face_pair::facing_cylinders", |k| {
        Ok(
            kernel_facing_cylinders(k, &cylinder(a), &cylinder(b))?.map(|f| FacingCylinders {
                thickness: f.thickness,
                mid_radius: f.mid_radius,
                axis_point: [f.axis_point.x, f.axis_point.y, f.axis_point.z],
                axis: [f.axis.x, f.axis.y, f.axis.z],
            }),
        )
    })
}

fn vec(a: [f64; 3]) -> Vec3 {
    Vec3::new(a[0], a[1], a[2])
}

/// Whether a cylindrical face's material lies toward its axis (through `axis_point` along `axis`),
/// read at the face's `point` from its `outward` normal there
/// ([`solidcore::surface::face_pair_cylinder::material_toward_axis`]).
///
/// # Errors
/// The kernel's refusal ([`crate::Error::Status`]) of a point or vector that is not finite, an
/// axis or normal of zero length, or a point on the axis.
pub fn material_toward_axis(
    axis_point: [f64; 3],
    axis: [f64; 3],
    point: [f64; 3],
    outward: [f64; 3],
) -> Result<bool> {
    run("face_pair::material_toward_axis", |_| {
        kernel_material_toward_axis(vec(axis_point), vec(axis), vec(point), vec(outward))
    })
}

/// The unit directions that stay on a cylindrical face from one of its points, where a curve runs
/// along `along`: both ways along `axis`, then both ways round it unless the curve runs along the
/// axis ([`solidcore::surface::face_pair_cylinder::round_directions`]).
///
/// # Errors
/// The kernel's refusal ([`crate::Error::Status`]) of a vector that is not finite, or an axis of
/// zero length.
pub fn round_directions(axis: [f64; 3], along: [f64; 3]) -> Result<Vec<[f64; 3]>> {
    run("face_pair::round_directions", |_| {
        Ok(kernel_round_directions(vec(axis), vec(along))?
            .into_iter()
            .map(|d| [d.x, d.y, d.z])
            .collect())
    })
}

/// Cylinder `b`'s axial range `range` read in cylinder `a`'s axial coordinates, low end first
/// ([`solidcore::surface::face_pair_cylinder::axial_range_in`]).
///
/// # Errors
/// The kernel's refusal ([`crate::Error::Status`]) of an axis point, axis or range end that is not
/// finite, or an axis of zero length.
pub fn axial_range_in(a: &CylinderFace, b: &CylinderFace, range: [f64; 2]) -> Result<[f64; 2]> {
    run("face_pair::axial_range_in", |_| {
        kernel_axial_range_in(&cylinder(a), &cylinder(b), range)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cylinders_side_directions_and_range_through_ops() {
        // Axis z: at (10, 0, 5) a normal +x is an outside, -x a bore.
        let (o, z) = ([0.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
        assert!(material_toward_axis(o, z, [10.0, 0.0, 5.0], [1.0, 0.0, 0.0]).unwrap());
        assert!(!material_toward_axis(o, z, [10.0, 0.0, 5.0], [-1.0, 0.0, 0.0]).unwrap());
        assert!(material_toward_axis(o, z, [0.0, 0.0, 5.0], [1.0, 0.0, 0.0]).is_err());
        // At (10, 0, 0) on the radius-10 cylinder about z, an edge along +y: the face's tangent
        // plane there is spanned by z and y (both at right angles to the radial +x).
        assert_eq!(
            round_directions([0.0, 0.0, 2.0], [0.0, 3.0, 0.0]).unwrap(),
            vec![
                [0.0, 0.0, 1.0],
                [0.0, 0.0, -1.0],
                [0.0, 1.0, 0.0],
                [0.0, -1.0, 0.0]
            ]
        );
        assert!(round_directions([0.0; 3], [0.0, 1.0, 0.0]).is_err());
        // b from z = 50 downward: its 10..30 is a's 20..40.
        let a = CylinderFace {
            axis_point: o,
            axis: z,
            radius: 10.0,
            material_toward_axis: true,
        };
        let b = CylinderFace {
            axis_point: [0.0, 0.0, 50.0],
            axis: [0.0, 0.0, -1.0],
            radius: 8.0,
            material_toward_axis: false,
        };
        assert_eq!(axial_range_in(&a, &b, [10.0, 30.0]).unwrap(), [20.0, 40.0]);
        assert!(axial_range_in(&a, &b, [f64::NAN, 30.0]).is_err());
    }

    #[test]
    fn a_tube_wall_pairs_through_ops() {
        let outer = CylinderFace {
            axis_point: [0.0, 0.0, 0.0],
            axis: [0.0, 0.0, 1.0],
            radius: 10.0,
            material_toward_axis: true,
        };
        let bore = CylinderFace {
            axis_point: [0.0, 0.0, 9.0],
            axis: [0.0, 0.0, 2.0],
            radius: 8.0,
            material_toward_axis: false,
        };
        let pair = facing_cylinders(&outer, &bore).unwrap().unwrap();
        assert!(
            (pair.thickness - 2.0).abs() < 1e-12 && (pair.mid_radius - 9.0).abs() < 1e-12,
            "{pair:?}"
        );
        let air = CylinderFace {
            radius: 12.0,
            ..bore
        };
        assert_eq!(facing_cylinders(&outer, &air).unwrap(), None);
        let bad = CylinderFace {
            radius: f64::NAN,
            ..bore
        };
        assert!(facing_cylinders(&outer, &bad).is_err());
    }

    fn at(p: [f64; 3], n: [f64; 3]) -> OutwardPlane {
        OutwardPlane {
            point: p,
            outward: n,
        }
    }

    #[test]
    fn a_plates_faces_pair_through_ops() {
        let top = at([0.0, 0.0, 2.0], [0.0, 0.0, 1.0]);
        let bottom = at([5.0, -3.0, 0.0], [0.0, 0.0, -1.0]);
        let pair = facing_planes(&top, &bottom).unwrap().unwrap();
        assert!((pair.thickness - 2.0).abs() < 1e-12, "{pair:?}");
        assert!((pair.mid_point[2] - 1.0).abs() < 1e-12, "{pair:?}");
        assert!((pair.toward_first[2] - 1.0).abs() < 1e-12, "{pair:?}");
    }

    #[test]
    fn air_between_is_no_pair_and_nonsense_is_refused() {
        let low = at([0.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
        let high = at([0.0, 0.0, 2.0], [0.0, 0.0, -1.0]);
        assert_eq!(facing_planes(&low, &high).unwrap(), None);
        assert!(facing_planes(&low, &at([f64::NAN, 0.0, 0.0], [0.0, 0.0, -1.0])).is_err());
        assert!(facing_planes(&low, &at([0.0, 0.0, 0.0], [0.0, 0.0, 0.0])).is_err());
    }
}
