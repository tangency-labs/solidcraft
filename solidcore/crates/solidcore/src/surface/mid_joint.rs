//! Where two mid-planes meet, and moving a mid-sheet's boundary edge onto that line: the
//! geometry of a joint between two mid-surfaces. A mid-sheet is the part of a wall's halfway
//! plane that its two faces share; where it meets another wall's sheet, its boundary edge there
//! stands where the wall's faces end, not on the line the two halfway planes meet in. Moving
//! the edge onto that line, and meeting the edges beside it again, extends the sheet at an inside
//! corner (an L bracket) and trims it where it runs past the other wall (a T junction).
//!
//! A sheet is a polygon of straight edges in one plane, as a list of 3D corners in order.
//! Planes and lines meet within the kernel's angular tolerance ([`Tolerances::ang_tol_rad`]) and
//! lines that are meant to cross in a plane must come within its zone tolerance of one another
//! ([`Tolerances::zone_tol_3d`]).
//!
//! Not upstream: no counterpart in SMLib. Deviations: none. No recursion.
//!
//! [`Tolerances::ang_tol_rad`]: crate::kernel::Tolerances::ang_tol_rad
//! [`Tolerances::zone_tol_3d`]: crate::kernel::Tolerances::zone_tol_3d

use crate::error::{Result, SolidError};
use crate::geom::vector3d::{Point3, Vec3};
use crate::kernel::Kernel;
use crate::surface::face_pair::OutwardPlane;

const MEET: &str = "surface::mid_joint::planes_meeting";
const MOVE: &str = "surface::mid_joint::move_edge_onto_line";

/// The most corners a polygon may have.
pub const MAX_CORNERS: usize = 100_000;

/// A line: a point on it and its unit direction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Line3 {
    /// A point on the line.
    pub point: Point3,
    /// The line's unit direction.
    pub direction: Vec3,
}

fn finite(v: Vec3) -> bool {
    v.x.is_finite() && v.y.is_finite() && v.z.is_finite()
}

/// The line two planes meet in, or `None` when they are parallel within the angular tolerance
/// (whether they face the same way or opposite ways), or so near parallel that no point of the
/// line can be placed within the zone tolerance of both at this precision.
///
/// # Errors
/// [`SolidError::InvalidInput`] for a point or normal that is not finite, or a normal of zero
/// length.
pub fn planes_meeting(
    kernel: &Kernel,
    a: &OutwardPlane,
    b: &OutwardPlane,
) -> Result<Option<Line3>> {
    if !(finite(a.point) && finite(b.point) && finite(a.outward) && finite(b.outward)) {
        return Err(SolidError::InvalidInput {
            operation: MEET,
            reason: "a plane's point and normal must be finite",
        });
    }
    let (ua, ub) = (a.outward.unitize()?, b.outward.unitize()?);
    let along = ua.cross(ub);
    let sine_squared = along.length_squared();
    let tol = kernel.tolerances.ang_tol_rad().sin();
    // Parallel within the angular tolerance, or to working precision (no direction to unitize).
    if sine_squared <= tol * tol || sine_squared < crate::math::EFF_ZERO_SQ {
        return Ok(None);
    }
    // The point of both planes nearest the origin: with n = ua × ub and d the planes' offsets,
    // p = (da (ub × n) + db (n × ua)) / |n|^2, for p . ua = da (ua . (ub × n) = n . n), p . ub = db
    // and p . n = 0. It is built from n alone, not from 1 - (ua . ub)^2, which loses its digits
    // to cancellation as the planes near parallel.
    let (da, db) = (a.point.dot(ua), b.point.dot(ub));
    let point = ((ub.cross(along) * da) + (along.cross(ua) * db)) * (1.0 / sine_squared);
    // Planes so near parallel (under a tiny angular tolerance) that rounding moves the point off
    // either plane by more than the zone tolerance meet in no line this precision can place.
    let zone = kernel.tolerances.zone_tol_3d;
    if point.distance_along(a.point, ua).abs() > zone
        || point.distance_along(b.point, ub).abs() > zone
    {
        return Ok(None);
    }
    Ok(Some(Line3 {
        point,
        direction: along.unitize()?,
    }))
}

/// Where two lines in one plane cross, or `None` when they are parallel within the angular
/// tolerance.
fn crossing(
    kernel: &Kernel,
    (p, d): (Point3, Vec3),
    (q, e): (Point3, Vec3),
) -> Result<Option<Point3>> {
    let n = d.cross(e);
    let n2 = n.length_squared();
    let sine = kernel.tolerances.ang_tol_rad().sin();
    if n2 <= sine * sine {
        return Ok(None);
    }
    let r = q - p;
    let (t, s) = (r.cross(e).dot(n) / n2, r.cross(d).dot(n) / n2);
    let (on_first, on_second) = (p + (d * t), q + (e * s));
    // Lines meant to cross in a plane do; skew ones are not this polygon's plane.
    if (on_first - on_second).length() > kernel.tolerances.zone_tol_3d {
        return Err(SolidError::InvalidInput {
            operation: MOVE,
            reason: "the line is not in the polygon's plane",
        });
    }
    Ok(Some(on_first))
}

/// The index after `i` among `n`, going round.
fn next(i: usize, n: usize) -> usize {
    if i + 1 >= n {
        0
    } else {
        i + 1
    }
}

/// The index before `i` among `n`, going round.
fn before(i: usize, n: usize) -> usize {
    if i == 0 {
        n.saturating_sub(1)
    } else {
        i - 1
    }
}

/// Moves edge `edge` of `polygon` (from corner `edge` to the corner after it) onto `line`, and
/// meets the edges beside it again there: the two corners of the edge slide along their
/// neighbouring edges to the line. The sheet grows where the line lies outside the polygon and
/// shrinks where it lies inside. `None` when the edge is not parallel to the line within the
/// angular tolerance, or a neighbouring edge is.
///
/// # Errors
/// [`SolidError::InvalidInput`] for fewer than three or more than [`MAX_CORNERS`] corners, a
/// corner or line that is not finite, an edge index past the corners, two corners at one place
/// where an edge's direction is needed, or a line that is not in the polygon's plane (it does
/// not meet a neighbouring edge within the zone tolerance).
pub fn move_edge_onto_line(
    kernel: &Kernel,
    polygon: &[Point3],
    edge: usize,
    line: &Line3,
) -> Result<Option<Vec<Point3>>> {
    let bad = |reason| SolidError::InvalidInput {
        operation: MOVE,
        reason,
    };
    let n = polygon.len();
    if !(3..=MAX_CORNERS).contains(&n) {
        return Err(bad("a polygon has three or more corners, and not too many"));
    }
    if !polygon.iter().all(|p| finite(*p)) || !finite(line.point) || !finite(line.direction) {
        return Err(bad("corners and the line must be finite"));
    }
    let corner = |i: usize| {
        polygon
            .get(i)
            .copied()
            .ok_or_else(|| bad("an edge past the corners"))
    };
    let (k, k1) = (edge, next(edge, n));
    let (a, b) = (corner(k)?, corner(k1)?);
    let along = line.direction.unitize()?;
    let sine = kernel.tolerances.ang_tol_rad().sin();
    if (b - a).unitize()?.cross(along).length() > sine {
        return Ok(None);
    }
    let (p, q) = (corner(before(k, n))?, corner(next(k1, n))?);
    let line_of =
        |from: Point3, to: Point3| -> Result<(Point3, Vec3)> { Ok((from, (to - from).unitize()?)) };
    let target = (line.point, along);
    let Some(new_a) = crossing(kernel, line_of(p, a)?, target)? else {
        return Ok(None);
    };
    let Some(new_b) = crossing(kernel, line_of(b, q)?, target)? else {
        return Ok(None);
    };
    let moved = polygon
        .iter()
        .enumerate()
        .map(|(i, c)| {
            if i == k {
                new_a
            } else if i == k1 {
                new_b
            } else {
                *c
            }
        })
        .collect();
    Ok(Some(moved))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::KernelOptions;

    fn kernel() -> Kernel {
        Kernel::new(&KernelOptions::default())
    }

    fn v(x: f64, y: f64, z: f64) -> Vec3 {
        Vec3::new(x, y, z)
    }

    fn plane(point: Vec3, outward: Vec3) -> OutwardPlane {
        OutwardPlane { point, outward }
    }

    /// The area of a planar polygon of 3D corners.
    fn area(corners: &[Point3]) -> f64 {
        let mut sum = Vec3::new(0.0, 0.0, 0.0);
        for i in 0..corners.len() {
            sum += corners[i].cross(corners[(i + 1) % corners.len()]);
        }
        sum.length() * 0.5
    }

    #[test]
    fn two_perpendicular_planes_meet_in_the_line_through_both() {
        // y = 1 and x = 1 meet along the z direction through (1, 1, anything).
        let k = kernel();
        let a = plane(v(5.0, 1.0, 3.0), v(0.0, 1.0, 0.0));
        let b = plane(v(1.0, -4.0, 9.0), v(2.0, 0.0, 0.0));
        let line = planes_meeting(&k, &a, &b).unwrap().unwrap();
        assert!(line.direction.z.abs() > 1.0 - 1e-12, "{line:?}");
        assert!(line.direction.x.abs() < 1e-12 && line.direction.y.abs() < 1e-12);
        assert!(
            (line.point.x - 1.0).abs() < 1e-12 && (line.point.y - 1.0).abs() < 1e-12,
            "{line:?}"
        );
        // At an angle: x + y = 2 and x - y = 0 meet where x = y = 1.
        let s = 0.5_f64.sqrt();
        let c = plane(v(1.0, 1.0, 0.0), v(s, s, 0.0));
        let d = plane(v(7.0, 7.0, 0.0), v(s, -s, 0.0));
        let line = planes_meeting(&k, &c, &d).unwrap().unwrap();
        assert!(
            (line.point.x - 1.0).abs() < 1e-12 && (line.point.y - 1.0).abs() < 1e-12,
            "{line:?}"
        );
    }

    #[test]
    fn parallel_planes_do_not_meet_whichever_way_they_face() {
        let k = kernel();
        let a = plane(v(0.0, 0.0, 0.0), v(0.0, 0.0, 1.0));
        assert_eq!(
            planes_meeting(&k, &a, &plane(v(0.0, 0.0, 2.0), v(0.0, 0.0, 1.0))).unwrap(),
            None
        );
        assert_eq!(
            planes_meeting(&k, &a, &plane(v(0.0, 0.0, 2.0), v(0.0, 0.0, -1.0))).unwrap(),
            None
        );
    }

    #[test]
    fn nearly_parallel_planes_under_no_angular_tolerance_meet_on_both_or_not_at_all() {
        let mut k = kernel();
        k.tolerances.ang_tol_deg = 0.0;
        let zone = k.tolerances.zone_tol_3d;
        // x = 1 and the plane through (1, 0, 0) with normal (1, 2^-27, 0): they meet in the line
        // through (1, 0, 0) along z, which is its point nearest the origin. (1 - c^2 rounds to
        // 0 here, and the old form gave the origin, 1 off the first plane.)
        let a = plane(v(1.0, 0.0, 0.0), v(1.0, 0.0, 0.0));
        let b = plane(v(1.0, 0.0, 0.0), v(1.0, 2.0_f64.powi(-27), 0.0));
        let line = planes_meeting(&k, &a, &b).unwrap().unwrap();
        assert!((line.point - v(1.0, 0.0, 0.0)).length() < 1e-12, "{line:?}");
        assert!(line.direction.z.abs() > 1.0 - 1e-12, "{line:?}");
        // Whatever the tilt down to the last bits, a line given lies on both planes.
        for e in 20..=60 {
            let t = 2.0_f64.powi(-e);
            let c = plane(v(3.0, -7.0, 2.0), v(1.0, 1.0, 0.0));
            let d = plane(v(-5.0, 4.0, 9.0), v(1.0 + t, 1.0, 0.0));
            if let Some(l) = planes_meeting(&k, &c, &d).unwrap() {
                for p in [&c, &d] {
                    let n = p.outward.unitize().unwrap();
                    assert!(
                        l.point.distance_along(p.point, n).abs() <= zone,
                        "2^-{e}: {l:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn an_l_brackets_two_sheets_meet_on_their_joint_line_and_total_the_closed_form() {
        // A 2 mm L bracket, legs 40 and 30 mm outside, 20 mm wide (profile (0,0) (40,0) (40,2)
        // (2,2) (2,30) (0,30), extruded along z). Leg A's halfway plane is y = 1: its faces
        // y = 0 (x 0..40) and y = 2 (x 2..40) share x 2..40. Leg B's is x = 1: faces x = 0 (y
        // 0..30) and x = 2 (y 2..30) share y 2..30.
        let k = kernel();
        let sheet_a = [
            v(2.0, 1.0, 0.0),
            v(40.0, 1.0, 0.0),
            v(40.0, 1.0, 20.0),
            v(2.0, 1.0, 20.0),
        ];
        let sheet_b = [
            v(1.0, 2.0, 0.0),
            v(1.0, 30.0, 0.0),
            v(1.0, 30.0, 20.0),
            v(1.0, 2.0, 20.0),
        ];
        let joint = planes_meeting(
            &k,
            &plane(v(0.0, 1.0, 0.0), v(0.0, 1.0, 0.0)),
            &plane(v(1.0, 0.0, 0.0), v(1.0, 0.0, 0.0)),
        )
        .unwrap()
        .unwrap();
        // Before: 38 x 20 and 28 x 20.
        assert!((area(&sheet_a) - 760.0).abs() < 1e-9 && (area(&sheet_b) - 560.0).abs() < 1e-9);
        // The edges at x = 2 (corner 3 to corner 0) and y = 2 (corner 3 to corner 0) move on.
        let a = move_edge_onto_line(&k, &sheet_a, 3, &joint)
            .unwrap()
            .unwrap();
        let b = move_edge_onto_line(&k, &sheet_b, 3, &joint)
            .unwrap()
            .unwrap();
        assert!(
            a.iter().any(|p| (p.x - 1.0).abs() < 1e-12
                && (p.y - 1.0).abs() < 1e-12
                && p.z.abs() < 1e-12),
            "{a:?}"
        );
        assert!(
            b.iter().any(|p| (p.x - 1.0).abs() < 1e-12
                && (p.y - 1.0).abs() < 1e-12
                && (p.z - 20.0).abs() < 1e-12),
            "{b:?}"
        );
        assert!((area(&a) - 780.0).abs() < 1e-9, "{}", area(&a));
        assert!((area(&b) - 580.0).abs() < 1e-9, "{}", area(&b));
        assert!((area(&a) + area(&b) - 1360.0).abs() < 1e-9);
    }

    #[test]
    fn an_edge_past_the_line_is_trimmed_back_to_it() {
        // A sheet 40 x 20 in z = 0; a wall stands on the line x = 5: the sheet is trimmed to 35.
        let k = kernel();
        let sheet = [
            v(0.0, 0.0, 0.0),
            v(40.0, 0.0, 0.0),
            v(40.0, 20.0, 0.0),
            v(0.0, 20.0, 0.0),
        ];
        let wall = Line3 {
            point: v(5.0, 0.0, 0.0),
            direction: v(0.0, 1.0, 0.0),
        };
        let trimmed = move_edge_onto_line(&k, &sheet, 3, &wall).unwrap().unwrap();
        assert!((area(&trimmed) - 700.0).abs() < 1e-9, "{trimmed:?}");
    }

    #[test]
    fn an_edge_that_cannot_move_onto_the_line_is_none_and_nonsense_is_refused() {
        let k = kernel();
        let sheet = [
            v(0.0, 0.0, 0.0),
            v(40.0, 0.0, 0.0),
            v(40.0, 20.0, 0.0),
            v(0.0, 20.0, 0.0),
        ];
        // The edge at x = 0 is not parallel to a line along x.
        let across = Line3 {
            point: v(5.0, 0.0, 0.0),
            direction: v(1.0, 0.0, 0.0),
        };
        assert_eq!(move_edge_onto_line(&k, &sheet, 3, &across).unwrap(), None);
        // A line out of the sheet's plane does not meet its neighbouring edges.
        let off = Line3 {
            point: v(5.0, 0.0, 3.0),
            direction: v(0.0, 1.0, 0.0),
        };
        assert!(move_edge_onto_line(&k, &sheet, 3, &off).is_err());
        // Index past the corners, too few corners, non-finite input.
        let on = Line3 {
            point: v(5.0, 0.0, 0.0),
            direction: v(0.0, 1.0, 0.0),
        };
        assert!(move_edge_onto_line(&k, &sheet, 4, &on).is_err());
        assert!(move_edge_onto_line(&k, &sheet[..2], 0, &on).is_err());
        let nan = [v(f64::NAN, 0.0, 0.0), sheet[1], sheet[2], sheet[3]];
        assert!(move_edge_onto_line(&k, &nan, 3, &on).is_err());
        let nan_line = Line3 {
            point: v(0.0, f64::INFINITY, 0.0),
            direction: v(0.0, 1.0, 0.0),
        };
        assert!(move_edge_onto_line(&k, &sheet, 3, &nan_line).is_err());
    }
}
