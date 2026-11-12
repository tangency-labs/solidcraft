//! The corner where three extended surfaces meet, one of them a screw wall.
//!
//! An edge of an offset ends where its curve crosses a neighbour's extended surface.
//! Beside a thread's wall that crossing can be shallow: the section of a flank by an end
//! cap crosses the root or crest cylinder a few degrees off its tangent, so the
//! intersection curve's own distance δ from the wall moves its crossing about δ / sin α
//! along it (1.7e-5 for δ = 8.5e-7 at α = 2.9° on an M10×1.5 thread), while the other
//! edges at the corner, crossing well, end at the true corner. The corner is a regular
//! root of the three surfaces' distance functions: it is solved here by Newton's method
//! on the surfaces' definitions, and each edge ends at its curve's point nearest it, so
//! its end is within its own deviation δ of the corner, not δ / sin α.
//! Deviations: new construction; at most 32 Newton steps, no recursion.

use super::{Point3, Result, Surface, Vec3};
use crate::geom::extent2d::Extent2d;
use crate::kernel::Kernel;
use crate::nurbs::knots::Side;
use crate::surface::procedural_blend::NetScope;
use crate::surface::surface::p4::NoObject;
use crate::surface::surface::p5::DropPointOptions;

/// Newton steps before the corner is given up (quadratic convergence needs a handful).
const MAX_STEPS: usize = 32;
/// The least |n1 · (n2 × n3)| of the three unit normals: below it the corner is not
/// a regular root and the edges keep their crossings.
const MIN_DETERMINANT: f64 = 1e-8;
/// The relative residual at which a point is on a surface (well below any tolerance
/// and above the definitions' evaluation rounding).
const RESIDUAL: f64 = 1e-10;

/// Whether `surface` is a screw-invariant swept wall: a thread's flank kept on its
/// definition, or its exact offset.
pub(crate) fn is_screw_wall(surface: &Surface) -> bool {
    surface
        .procedural_sweep()
        .is_some_and(|sweep| sweep.definition().screw().is_some())
}

/// The point within `reach` of `start` on all three `surfaces` (each over its domain),
/// evaluated on their definitions, or `None` where Newton's method does not converge
/// there to a regular corner (normals nearly dependent, a drop that fails, a step
/// beyond `reach`).
///
/// # Errors
///
/// Those of the surfaces' drops and evaluations.
pub(crate) fn corner(
    k: &Kernel,
    surfaces: [(&Surface, &Extent2d); 3],
    start: Point3,
    reach: f64,
) -> Result<Option<Point3>> {
    let _exact = NetScope::exact(k);
    let mut x = start;
    for _ in 0..MAX_STEPS {
        let mut rows = [(Vec3::default(), 0.0_f64); 3];
        let mut residual = 0.0_f64;
        for ((surface, domain), row) in surfaces.iter().zip(rows.iter_mut()) {
            let drop =
                surface.drop_point::<NoObject>(k, x, domain, &DropPointOptions::default())?;
            if !drop.success {
                return Ok(None);
            }
            let foot = surface.evaluate_point(k, drop.result_uv)?;
            let normal = surface.evaluate_normal(k, drop.result_uv, Side::Left, Side::Left)?;
            let length = normal.length();
            if !length.is_finite() || length <= 0.0 {
                return Ok(None);
            }
            let normal = normal / length;
            residual = residual.max((x - foot).length());
            *row = (normal, normal.dot(x - foot));
        }
        let scale = 1.0 + x.length();
        if residual <= RESIDUAL * scale {
            return Ok(((x - start).length() <= reach).then_some(x));
        }
        let [(n1, d1), (n2, d2), (n3, d3)] = rows;
        let (c1, c2, c3) = (n2.cross(n3), n3.cross(n1), n1.cross(n2));
        let determinant = n1.dot(c1);
        if !determinant.is_finite() || determinant.abs() < MIN_DETERMINANT {
            return Ok(None);
        }
        x -= (c1 * d1 + c2 * d2 + c3 * d3) / determinant;
        if !(x - start).length().is_finite() || (x - start).length() > reach {
            return Ok(None);
        }
    }
    Ok(None)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // test assertions
mod tests {
    use super::*;
    use crate::math::Vec2;
    use crate::surface::plane::Plane;

    fn plane(origin: Point3, x: Vec3, y: Vec3) -> Surface {
        Surface::from(Plane::new(
            origin,
            x,
            y,
            Vec2::new(1.0, 1.0),
            &Extent2d::from_bounds(-10.0, -10.0, 10.0, 10.0),
        ))
    }

    /// Three planes, one tilted 3° off another: the corner is their one common point,
    /// found from a start 1e-4 off it, and none where two planes are parallel or the
    /// corner is beyond the reach.
    #[test]
    fn the_corner_of_three_planes_is_their_common_point() {
        let k = Kernel::default();
        let tilt = 3.0_f64.to_radians();
        let a = plane(Point3::new(0.0, 0.0, 1.0), Vec3::X, Vec3::Y);
        let b = plane(Point3::new(2.0, 0.0, 0.0), Vec3::Y, Vec3::Z);
        let c = plane(
            Point3::new(0.0, 3.0, 0.0),
            Vec3::new(tilt.cos(), tilt.sin(), 0.0),
            Vec3::Z,
        );
        let domain = |s: &Surface| s.natural_uv_domain_with_nurb(&k).unwrap();
        let (da, db, dc) = (domain(&a), domain(&b), domain(&c));
        let exact = Point3::new(2.0, 3.0 + 2.0 * tilt.tan(), 1.0);
        let start = exact + Vec3::new(1e-4, -1e-4, 1e-4);
        let found = corner(&k, [(&a, &da), (&b, &db), (&c, &dc)], start, 1e-3)
            .unwrap()
            .unwrap();
        assert!((found - exact).length() < 1e-12, "{found:?}");
        assert!(corner(&k, [(&a, &da), (&b, &db), (&c, &dc)], start, 1e-5)
            .unwrap()
            .is_none());
        let parallel = plane(Point3::new(0.0, 0.0, 2.0), Vec3::X, Vec3::Y);
        let dp = domain(&parallel);
        assert!(
            corner(&k, [(&a, &da), (&b, &db), (&parallel, &dp)], start, 1e-3)
                .unwrap()
                .is_none()
        );
        assert!(!is_screw_wall(&a));
    }
}
