//! A circular edge: the circle its curve lies on, and which way along the circle's axis is out
//! of the body the edge bounds. What a pick of a hole's or a boss's rim stands for.
//!
//! Upstream: none (not in SMLib).
//!
//! | Rust                     | Upstream |
//! |--------------------------|----------|
//! | [`edge_circle`]          | (none)   |
//! | [`axis_out_of_material`] | (none)   |
//!
//! Deviations: no upstream counterpart.

use crate::curve::curve::CurveKind;
use crate::curve::ellipse::Ellipse;
use crate::error::{Result, SolidError};
use crate::kernel::Kernel;
use crate::math::{Point3, Vec3};
use crate::nurbs::math::MODEL_TOL;
use crate::topology::{edge, Brep, EdgeId};

const OP: &str = "query::circle_edge";

/// The circle an edge's curve lies on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EdgeCircle {
    /// The circle's centre.
    pub center: Point3,
    /// The unit normal of the circle's plane (its axis), in the curve's own sense.
    pub axis: Vec3,
    /// The circle's radius.
    pub radius: f64,
}

/// The circle edge `e`'s curve lies on, read from the exact curve: a circle, or a rational
/// quadratic spline that is one ([`Ellipse::is_nurb_curve_circle`]). `None` for any other curve
/// and for an edge without a curve.
///
/// # Errors
///
/// A dangling edge; those of [`Ellipse::is_nurb_curve_circle`].
pub fn edge_circle(k: &Kernel, b: &Brep, e: EdgeId) -> Result<Option<EdgeCircle>> {
    let Some(curve) = edge::curve(b, e)? else {
        return Ok(None);
    };
    let circle = match curve.kind() {
        CurveKind::Ellipse(c) if c.is_circle_class() => Some(c.copy(k)?),
        CurveKind::BSpline(_) => Ellipse::is_nurb_curve_circle(k, curve, None)?,
        _ => None,
    };
    Ok(circle.map(|c| {
        let c = c.canonical();
        EdgeCircle {
            center: c.position.origin(),
            axis: c.position.z_axis(),
            radius: c.radius_x,
        }
    }))
}

/// A circle's `axis` signed to point out of the material the circle bounds, from the outward
/// unit normals of the faces along its edge, each with its proved bound: the way their sum leans
/// along the axis. A cap across the axis leans along it fully, a wall around it not at all, so a
/// hole's or a boss's rim points out of its cap. `None` when the sum leans neither way by more
/// than the bounds and the model tolerance (a rim between two walls).
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for an axis of no length or a value that is not finite.
pub fn axis_out_of_material(axis: Vec3, normals: &[(Vec3, f64)]) -> Result<Option<Vec3>> {
    let axis = axis.unitize()?;
    let (mut lean, mut bound) = (0.0, MODEL_TOL);
    for &(n, b) in normals {
        lean += n.dot(axis);
        bound += b;
    }
    if !(lean.is_finite() && bound.is_finite()) {
        return Err(SolidError::invalid_input(
            OP,
            "a normal or its bound is not finite",
        ));
    }
    Ok(if lean > bound {
        Some(axis)
    } else if lean < -bound {
        Some(-axis)
    } else {
        None
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A cylinder r 5, h 10 about +z from the origin: its two rims are circles of radius 5 about
    /// the z axis at z = 0 and z = 10; its seam is no circle.
    #[test]
    fn a_cylinder_rims_are_circles_about_its_axis() {
        let mut k = Kernel::default();
        let b = crate::primitive::api_primitives::create_cylinder(&mut k, Point3::ZERO, 5.0, 10.0)
            .unwrap();
        let mut heights = Vec::new();
        let mut others = 0;
        for e in b.edge_list().iter().copied() {
            match edge_circle(&k, &b, e).unwrap() {
                Some(c) => {
                    assert!((c.radius - 5.0).abs() < 1e-9, "{c:?}");
                    assert!(c.center.x.abs() < 1e-9 && c.center.y.abs() < 1e-9, "{c:?}");
                    assert!((c.axis.z.abs() - 1.0).abs() < 1e-9, "{c:?}");
                    heights.push(c.center.z);
                }
                None => others += 1,
            }
        }
        heights.sort_by(f64::total_cmp);
        assert_eq!(heights.len(), 2, "{heights:?}");
        assert!(
            heights[0].abs() < 1e-9 && (heights[1] - 10.0).abs() < 1e-9,
            "{heights:?}"
        );
        assert_eq!(others, b.edge_list().len() - 2);
    }

    /// A rim on a cap leans with the cap's normal whichever way the axis is given; a rim between
    /// two walls (normals square to the axis) leans neither way; a cone's normal leans part way.
    #[test]
    fn a_circle_axis_points_out_of_its_cap() {
        let (up, down) = (Vec3::new(0.0, 0.0, 1.0), Vec3::new(0.0, 0.0, -1.0));
        let wall = (Vec3::new(1.0, 0.0, 0.0), 1e-15);
        assert_eq!(
            axis_out_of_material(up, &[(up, 1e-15), wall]).unwrap(),
            Some(up)
        );
        assert_eq!(
            axis_out_of_material(down * 3.0, &[(up, 1e-15), wall]).unwrap(),
            Some(up)
        );
        assert_eq!(
            axis_out_of_material(up, &[(down, 1e-15), wall]).unwrap(),
            Some(down)
        );
        assert_eq!(
            axis_out_of_material(up, &[wall, (Vec3::new(-1.0, 0.0, 0.0), 1e-15)]).unwrap(),
            None
        );
        let cone = (Vec3::new(0.6, 0.0, 0.8), 1e-15);
        assert_eq!(axis_out_of_material(down, &[cone, wall]).unwrap(), Some(up));
        assert!(axis_out_of_material(Vec3::ZERO, &[wall]).is_err());
        assert!(axis_out_of_material(up, &[(Vec3::new(f64::NAN, 0.0, 0.0), 0.0)]).is_err());
    }
}
