//! Where two mid-planes meet, and moving a mid-sheet's boundary edge onto that line: the
//! geometry of a joint between two mid-surfaces. A mid-sheet is the part of a wall's halfway
//! plane that its two faces share; where it meets another wall's sheet, its boundary edge there
//! stands where the wall's faces end, not on the line the two halfway planes meet in. Moving
//! the edge onto that line, and meeting the edges beside it again, extends the sheet at an inside
//! corner (an L bracket) and trims it where it runs past the other wall (a T junction).
//!
//! Where a flat wall meets a round one the joint is curved: the halfway plane meets the halfway
//! cylinder in a circle (the plane square to the axis: a tube standing on a plate, a ring round a
//! tube), an ellipse (across it at a slant), or lines (along it: a rib on a tube)
//! ([`plane_meets_cylinder`]). A bore through a flat wall crosses its halfway plane in the same
//! curve, the exact edge of the hole the sheet carries. An ellipse is read in a sheet's frame as
//! four exact rational quadratic quarters ([`ellipse_in_frame`]); a round sheet's end is carried
//! along the axis to the joint ([`along_axis_to_plane`], [`onto_cylinder`]).
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
use crate::surface::mid_sheet::{in_frame, Frame};

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
    // either plane by more than the zone tolerance meet in no line this precision can place; nor
    // do planes whose point overflows (a NaN residual fails the test).
    let zone = kernel.tolerances.zone_tol_3d;
    if !(point.distance_along(a.point, ua).abs() <= zone
        && point.distance_along(b.point, ub).abs() <= zone)
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
    // Before `next(edge, n)`, whose `edge + 1` would overflow at `usize::MAX`.
    if edge >= n {
        return Err(bad("an edge past the corners"));
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

const CYLINDER: &str = "surface::mid_joint::plane_meets_cylinder";
const CONICS: &str = "surface::mid_joint::ellipse_in_frame";
const ALONG: &str = "surface::mid_joint::along_axis_to_plane";
const ONTO: &str = "surface::mid_joint::onto_cylinder";

/// An ellipse in space: its centre and two semi-axes at right angles (a circle when they are
/// equally long).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ellipse3 {
    /// The centre.
    pub centre: Point3,
    /// The first semi-axis: from the centre to the ellipse, the longer one.
    pub major: Vec3,
    /// The second semi-axis, at right angles to the first.
    pub minor: Vec3,
}

/// Where a plane meets a cylinder: what the halfway plane of a flat wall and the halfway
/// cylinder of a round wall have in common (the curve their joint lies on), or where a bore
/// crosses a flat wall's halfway plane (the edge of the hole the sheet carries).
#[derive(Debug, Clone, PartialEq)]
pub enum PlaneCylinder {
    /// The plane runs along the axis (within the angular tolerance): the lines it cuts the
    /// cylinder in, two, or one where it touches the cylinder (within the zone tolerance).
    Lines(Vec<Line3>),
    /// The plane crosses the axis: an ellipse whose minor semi-axis is the radius and whose
    /// major one is the radius over the cosine of the angle between the plane's normal and the
    /// axis. `square`: the plane is square to the axis within the angular tolerance, and the
    /// ellipse a circle of the radius.
    Ellipse {
        /// The curve.
        ellipse: Ellipse3,
        /// Whether the plane is square to the axis (the curve is a circle).
        square: bool,
    },
}

/// Where `plane` meets the cylinder of `radius` about the axis through `axis_point` along
/// `axis`. `None` when the plane runs along the axis farther from it than the radius (and the
/// zone tolerance).
///
/// Along the axis (the normal `n` and the unit axis `a` at right angles, within the angular
/// tolerance), the plane lies `d` from the axis and cuts the cylinder in the lines along `a`
/// through its foot on the plane moved `±√(r² − d²)` along `a × n`. Across it, the axis meets
/// the plane at the centre; the minor semi-axis `r` runs along `a × n` (any direction in the
/// plane when it is square) and the major one `r / |a · n|` along `n × (a × n)`: a point
/// `x·m + y·M` from the centre lies `√(x² + (a · n)² y²)` from the axis, `r` on the ellipse.
///
/// # Errors
/// [`SolidError::InvalidInput`] for a plane, axis or radius that is not finite, a radius that is
/// not positive, a normal or axis of zero length, or a crossing so shallow that the ellipse
/// overflows.
pub fn plane_meets_cylinder(
    kernel: &Kernel,
    plane: &OutwardPlane,
    axis_point: Point3,
    axis: Vec3,
    radius: f64,
) -> Result<Option<PlaneCylinder>> {
    let bad = |reason| SolidError::InvalidInput {
        operation: CYLINDER,
        reason,
    };
    if !(finite(plane.point)
        && finite(plane.outward)
        && finite(axis_point)
        && finite(axis)
        && radius.is_finite())
    {
        return Err(bad("the plane, the axis and the radius must be finite"));
    }
    if radius <= 0.0 {
        return Err(bad("the radius must be positive"));
    }
    let (n, a) = (plane.outward.unitize()?, axis.unitize()?);
    let cosine = a.dot(n);
    let sine = kernel.tolerances.ang_tol_rad().sin();
    let zone = kernel.tolerances.zone_tol_3d;
    if cosine.abs() <= sine {
        // Along the axis: the axis's direction in the plane, and the way across it there.
        let along = (a - (n * cosine)).unitize()?;
        let across = along.cross(n).unitize()?;
        let d = axis_point.distance_along(plane.point, n);
        if d.abs() > radius + zone {
            return Ok(None);
        }
        let foot = axis_point - (n * d);
        if d.abs() >= radius - zone {
            return Ok(Some(PlaneCylinder::Lines(vec![Line3 {
                point: foot,
                direction: along,
            }])));
        }
        let half = (radius * radius - d * d).sqrt();
        let lines = [half, -half]
            .into_iter()
            .map(|s| Line3 {
                point: foot + (across * s),
                direction: along,
            })
            .collect();
        return Ok(Some(PlaneCylinder::Lines(lines)));
    }
    // The axis meets the plane at the centre.
    let t = plane.point.distance_along(axis_point, n) / cosine;
    let centre = axis_point + (a * t);
    let square = a.cross(n).length() <= sine;
    let (minor, major) = if square {
        // Any direction in the plane: one at right angles to the normal, and the next.
        let other = if n.x.abs() < 0.9 {
            Vec3::new(1.0, 0.0, 0.0)
        } else {
            Vec3::new(0.0, 1.0, 0.0)
        };
        let m = n.cross(other).unitize()?;
        (m * radius, n.cross(m) * radius)
    } else {
        let m = a.cross(n).unitize()?;
        (m * radius, n.cross(m) * (radius / cosine.abs()))
    };
    if !(finite(centre) && finite(major)) {
        return Err(bad(
            "the plane crosses the cylinder too shallowly to place the ellipse",
        ));
    }
    Ok(Some(PlaneCylinder::Ellipse {
        ellipse: Ellipse3 {
            centre,
            major,
            minor,
        },
        square,
    }))
}

/// A rational quadratic arc in a plane frame's coordinates: from `a` to `b` with the control
/// point `apex` weighted `w` (the ends weighted 1). A quarter of an ellipse is one exactly.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Conic2 {
    /// Its start.
    pub a: [f64; 2],
    /// Its control point.
    pub apex: [f64; 2],
    /// Its end.
    pub b: [f64; 2],
    /// The control point's weight.
    pub w: f64,
}

/// `ellipse` read in `frame` (dropped onto the frame's plane along its normal, when it lies in
/// another, which keeps an ellipse an ellipse) as its four quarters, exact rational quadratic
/// arcs, running counter-clockwise in the frame's coordinates: from the end of one semi-axis to
/// the next, the control point at the corner of the box the two span, weighted `cos 45°`. Two
/// semi-axes `U` and `V` that are conjugate (a circle's two radii at right angles, mapped by any
/// affine map) give the ellipse `c + U cos t + V sin t`, which those quarters are.
///
/// # Errors
/// [`SolidError::InvalidInput`] for an ellipse or frame that is not finite, or an ellipse that
/// is a segment or a point in the frame (its plane at right angles to the frame's).
pub fn ellipse_in_frame(frame: &Frame, ellipse: &Ellipse3) -> Result<[Conic2; 4]> {
    if !(finite(ellipse.centre) && finite(ellipse.major) && finite(ellipse.minor)) {
        return Err(SolidError::InvalidInput {
            operation: CONICS,
            reason: "the ellipse must be finite",
        });
    }
    let c = in_frame(frame, ellipse.centre)?;
    // A vector's coordinates: those of its tip set at the frame's origin.
    let read = |v: Vec3| in_frame(frame, frame.origin + v);
    let (u, mut v) = (read(ellipse.major)?, read(ellipse.minor)?);
    let turn = u[0] * v[1] - u[1] * v[0];
    let size = (u[0].abs() + u[1].abs()).max(v[0].abs() + v[1].abs());
    if !(turn.abs() > size * size * 1e-12) {
        return Err(SolidError::InvalidInput {
            operation: CONICS,
            reason: "the ellipse is edge-on to the frame's plane",
        });
    }
    // Counter-clockwise: the second semi-axis a quarter turn on from the first.
    if turn < 0.0 {
        v = [-v[0], -v[1]];
    }
    let at = |s: f64, t: f64, d: [f64; 2], e: [f64; 2]| {
        [c[0] + s * d[0] + t * e[0], c[1] + s * d[1] + t * e[1]]
    };
    let ends = [
        at(1.0, 0.0, u, v),
        at(0.0, 1.0, u, v),
        at(-1.0, 0.0, u, v),
        at(0.0, -1.0, u, v),
    ];
    let apexes = [
        at(1.0, 1.0, u, v),
        at(-1.0, 1.0, u, v),
        at(-1.0, -1.0, u, v),
        at(1.0, -1.0, u, v),
    ];
    let w = std::f64::consts::FRAC_1_SQRT_2;
    let quarter = |k: usize| Conic2 {
        a: ends[k % 4],
        apex: apexes[k % 4],
        b: ends[(k + 1) % 4],
        w,
    };
    Ok([quarter(0), quarter(1), quarter(2), quarter(3)])
}

/// How far `point` lies from `plane` measured along `axis` (in the axis's units of length, signed:
/// positive where the plane lies ahead along the axis). `None` when the axis runs along the plane
/// within the angular tolerance.
///
/// # Errors
/// [`SolidError::InvalidInput`] for a point, plane or axis that is not finite, or a normal or
/// axis of zero length.
pub fn along_axis_to_plane(
    kernel: &Kernel,
    plane: &OutwardPlane,
    point: Point3,
    axis: Vec3,
) -> Result<Option<f64>> {
    if !(finite(plane.point) && finite(plane.outward) && finite(point) && finite(axis)) {
        return Err(SolidError::InvalidInput {
            operation: ALONG,
            reason: "the plane, the point and the axis must be finite",
        });
    }
    let (n, a) = (plane.outward.unitize()?, axis.unitize()?);
    let cosine = a.dot(n);
    if cosine.abs() <= kernel.tolerances.ang_tol_rad().sin() {
        return Ok(None);
    }
    Ok(Some(plane.point.distance_along(point, n) / cosine))
}

/// `point` moved straight out from (or in toward) the axis through `axis_point` along `axis`
/// onto the cylinder of `radius` about it: the same place along the axis and the same angle
/// round it.
///
/// # Errors
/// [`SolidError::InvalidInput`] for a point, axis or radius that is not finite, a radius that is
/// not positive, an axis of zero length, or a point on the axis (no way out from it).
pub fn onto_cylinder(axis_point: Point3, axis: Vec3, radius: f64, point: Point3) -> Result<Point3> {
    let bad = |reason| SolidError::InvalidInput {
        operation: ONTO,
        reason,
    };
    if !(finite(axis_point) && finite(axis) && finite(point) && radius.is_finite()) {
        return Err(bad("the point, the axis and the radius must be finite"));
    }
    if radius <= 0.0 {
        return Err(bad("the radius must be positive"));
    }
    let a = axis.unitize()?;
    let foot = axis_point + (a * point.distance_along(axis_point, a));
    let out = (point - foot)
        .unitize()
        .map_err(|_| bad("the point lies on the axis"))?;
    Ok(foot + (out * radius))
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
        // The plane through (M, M, 0) with normal (1, 1, 0) (M = f64::MAX) and z = 0 meet in the
        // line through (M, M, 0) along (1, -1, 0), but its point nearest the origin, (M, M, 0), is
        // built from the offset M·√2, which overflows: the point comes out (inf, inf, NaN), off
        // both planes by NaN. No point can be placed, so none is given.
        let far = plane(v(f64::MAX, f64::MAX, 0.0), v(1.0, 1.0, 0.0));
        let floor = plane(v(0.0, 0.0, 0.0), v(0.0, 0.0, 1.0));
        assert_eq!(planes_meeting(&k, &far, &floor).unwrap(), None);
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
        assert!(move_edge_onto_line(&k, &sheet, usize::MAX, &on).is_err());
        // The last edge (corner 3 back to corner 0) wraps round and is the one that moves.
        assert!(move_edge_onto_line(&k, &sheet, 3, &on).unwrap().is_some());
        assert!(move_edge_onto_line(&k, &sheet[..2], 0, &on).is_err());
        let nan = [v(f64::NAN, 0.0, 0.0), sheet[1], sheet[2], sheet[3]];
        assert!(move_edge_onto_line(&k, &nan, 3, &on).is_err());
        let nan_line = Line3 {
            point: v(0.0, f64::INFINITY, 0.0),
            direction: v(0.0, 1.0, 0.0),
        };
        assert!(move_edge_onto_line(&k, &sheet, 3, &nan_line).is_err());
    }

    /// The distance of `p` from the z axis.
    fn off_z(p: Point3) -> f64 {
        (p.x * p.x + p.y * p.y).sqrt()
    }

    #[test]
    fn a_plane_square_to_a_cylinder_meets_it_in_a_circle_of_its_radius() {
        // A tube's halfway cylinder (radius 9 about z) and a plate's halfway plane z = 2.
        let k = kernel();
        let plane = plane(v(5.0, 7.0, 2.0), v(0.0, 0.0, -1.0));
        let Some(PlaneCylinder::Ellipse { ellipse, square }) =
            plane_meets_cylinder(&k, &plane, v(0.0, 0.0, -30.0), v(0.0, 0.0, 3.0), 9.0).unwrap()
        else {
            panic!("no circle");
        };
        assert!(square);
        assert!(
            (ellipse.centre - v(0.0, 0.0, 2.0)).length() < 1e-12,
            "{ellipse:?}"
        );
        assert!((ellipse.major.length() - 9.0).abs() < 1e-12);
        assert!((ellipse.minor.length() - 9.0).abs() < 1e-12);
        assert!(ellipse.major.dot(ellipse.minor).abs() < 1e-12);
        assert!(ellipse.major.z.abs() < 1e-12 && ellipse.minor.z.abs() < 1e-12);
    }

    #[test]
    fn a_tilted_plane_meets_a_cylinder_in_an_ellipse_of_radius_over_cosine() {
        // A bore of radius 3 along z through the plane tilted 30 degrees about y through
        // (0, 0, 1): semi-axes 3 and 3 / cos 30, area 9 pi / cos 30.
        let k = kernel();
        let (s, c) = (30f64.to_radians().sin(), 30f64.to_radians().cos());
        let tilted = plane(v(0.0, 0.0, 1.0), v(s, 0.0, c));
        let Some(PlaneCylinder::Ellipse { ellipse, square }) =
            plane_meets_cylinder(&k, &tilted, v(0.0, 0.0, 0.0), v(0.0, 0.0, 1.0), 3.0).unwrap()
        else {
            panic!("no ellipse");
        };
        assert!(!square);
        assert!((ellipse.centre - v(0.0, 0.0, 1.0)).length() < 1e-12);
        assert!((ellipse.minor.length() - 3.0).abs() < 1e-12);
        assert!((ellipse.major.length() - 3.0 / c).abs() < 1e-12);
        // Every point of it lies on the cylinder and in the plane.
        for i in 0..16 {
            let t = f64::from(i) * std::f64::consts::TAU / 16.0;
            let p = ellipse.centre + (ellipse.major * t.cos()) + (ellipse.minor * t.sin());
            assert!((off_z(p) - 3.0).abs() < 1e-12, "{p:?}");
            assert!(
                p.distance_along(tilted.point, tilted.outward).abs() < 1e-12,
                "{p:?}"
            );
        }
    }

    #[test]
    fn a_plane_along_the_axis_meets_a_cylinder_in_two_lines_one_or_none() {
        // A rib's halfway plane x = 0 and a tube's halfway cylinder of radius 9 about z: the
        // lines along z through (0, 9) and (0, -9). x = 9 touches it; x = 10 misses it.
        let k = kernel();
        let rib = plane(v(0.0, 4.0, 0.0), v(1.0, 0.0, 0.0));
        let Some(PlaneCylinder::Lines(lines)) =
            plane_meets_cylinder(&k, &rib, v(0.0, 0.0, 5.0), v(0.0, 0.0, -1.0), 9.0).unwrap()
        else {
            panic!("no lines");
        };
        assert_eq!(lines.len(), 2);
        let mut ys: Vec<f64> = lines.iter().map(|l| l.point.y).collect();
        ys.sort_by(f64::total_cmp);
        assert!(
            (ys[0] + 9.0).abs() < 1e-12 && (ys[1] - 9.0).abs() < 1e-12,
            "{lines:?}"
        );
        assert!(lines
            .iter()
            .all(|l| l.point.x.abs() < 1e-12 && l.direction.z.abs() > 1.0 - 1e-12));
        let touching = plane(v(9.0, 0.0, 0.0), v(1.0, 0.0, 0.0));
        let one = plane_meets_cylinder(&k, &touching, v(0.0, 0.0, 0.0), v(0.0, 0.0, 1.0), 9.0);
        assert!(matches!(one.unwrap(), Some(PlaneCylinder::Lines(l)) if l.len() == 1));
        let apart = plane(v(10.0, 0.0, 0.0), v(1.0, 0.0, 0.0));
        let none = plane_meets_cylinder(&k, &apart, v(0.0, 0.0, 0.0), v(0.0, 0.0, 1.0), 9.0);
        assert_eq!(none.unwrap(), None);
        // Nonsense is refused.
        let zero = plane_meets_cylinder(&k, &rib, v(0.0, 0.0, 0.0), v(0.0, 0.0, 1.0), 0.0);
        assert!(zero.is_err());
        let nan = plane_meets_cylinder(&k, &rib, v(f64::NAN, 0.0, 0.0), v(0.0, 0.0, 1.0), 1.0);
        assert!(nan.is_err());
    }

    /// The rational quadratic arc's point at `t`.
    fn conic_at(q: &Conic2, t: f64) -> [f64; 2] {
        let (b0, b1, b2) = ((1.0 - t) * (1.0 - t), 2.0 * t * (1.0 - t) * q.w, t * t);
        let d = b0 + b1 + b2;
        [
            (b0 * q.a[0] + b1 * q.apex[0] + b2 * q.b[0]) / d,
            (b0 * q.a[1] + b1 * q.apex[1] + b2 * q.b[1]) / d,
        ]
    }

    #[test]
    fn an_ellipse_reads_in_a_frame_as_four_exact_quarters_counter_clockwise() {
        // The tilted bore's ellipse above (semi-axes 3 / cos 30 along x', 3 along y) read in the
        // frame of the plane z = 1 (axes x and -y, so that the minor axis turns clockwise there):
        // dropped onto it, the ellipse x² / (3 / cos 30 · cos 30)² + y² / 9 = 1, a circle of 3.
        let k = kernel();
        let (s, c) = (30f64.to_radians().sin(), 30f64.to_radians().cos());
        let tilted = plane(v(0.0, 0.0, 1.0), v(s, 0.0, c));
        let Some(PlaneCylinder::Ellipse { ellipse, .. }) =
            plane_meets_cylinder(&k, &tilted, v(0.0, 0.0, 0.0), v(0.0, 0.0, 1.0), 3.0).unwrap()
        else {
            panic!("no ellipse");
        };
        let frame = Frame {
            origin: v(1.0, 2.0, 1.0),
            x: v(1.0, 0.0, 0.0),
            y: v(0.0, -1.0, 0.0),
        };
        let quarters = ellipse_in_frame(&frame, &ellipse).unwrap();
        // Closed, each quarter ending where the next starts.
        for k in 0..4 {
            assert_eq!(quarters[k].b, quarters[(k + 1) % 4].a);
        }
        // Every point lies 3 from the centre, (-1, 2) in the frame.
        for q in &quarters {
            for i in 0..=8 {
                let [x, y] = conic_at(q, f64::from(i) / 8.0);
                let r = ((x + 1.0) * (x + 1.0) + (y - 2.0) * (y - 2.0)).sqrt();
                assert!((r - 3.0).abs() < 1e-12, "{q:?} {x} {y}");
            }
        }
        // Counter-clockwise: the shoelace sum of the quarters' ends and apexes is positive.
        let turn: f64 = quarters
            .iter()
            .map(|q| q.a[0] * q.b[1] - q.b[0] * q.a[1])
            .sum();
        assert!(turn > 0.0);
        // In the tilted plane's own frame, it is the ellipse itself: a point a quarter of the
        // way along the first quarter lies on x² / a² + y² / b² = 1.
        let own = Frame {
            origin: v(0.0, 0.0, 1.0),
            x: v(c, 0.0, -s),
            y: v(0.0, 1.0, 0.0),
        };
        let quarters = ellipse_in_frame(&own, &ellipse).unwrap();
        let (a, b) = (3.0 / c, 3.0);
        for q in &quarters {
            let [x, y] = conic_at(q, 0.25);
            assert!(
                (x * x / (a * a) + y * y / (b * b) - 1.0).abs() < 1e-12,
                "{x} {y}"
            );
        }
        // Edge-on to the frame's plane: refused.
        let side = Frame {
            origin: v(0.0, 0.0, 0.0),
            x: v(1.0, 0.0, 0.0),
            y: v(0.0, 0.0, 1.0),
        };
        let flat = Ellipse3 {
            centre: v(0.0, 0.0, 0.0),
            major: v(0.0, 2.0, 0.0),
            minor: v(0.0, 0.0, 0.0),
        };
        assert!(ellipse_in_frame(&side, &flat).is_err());
    }

    #[test]
    fn a_point_is_read_along_the_axis_to_a_plane_and_moved_onto_a_cylinder() {
        let k = kernel();
        // The tube's end at z = 4 and the plate's halfway plane z = 2: 2 back along +z.
        let half = plane(v(30.0, -30.0, 2.0), v(0.0, 0.0, 1.0));
        let d = along_axis_to_plane(&k, &half, v(10.0, 0.0, 4.0), v(0.0, 0.0, 5.0)).unwrap();
        assert!(d.is_some_and(|d| (d + 2.0).abs() < 1e-12), "{d:?}");
        // An axis along the plane never reaches it.
        let none = along_axis_to_plane(&k, &half, v(10.0, 0.0, 4.0), v(1.0, 0.0, 0.0)).unwrap();
        assert_eq!(none, None);
        // (6, 8, 4) is 10 from the z axis: on radius 9 it is (5.4, 7.2, 4).
        let p = onto_cylinder(v(0.0, 0.0, -7.0), v(0.0, 0.0, 2.0), 9.0, v(6.0, 8.0, 4.0)).unwrap();
        assert!((p - v(5.4, 7.2, 4.0)).length() < 1e-12, "{p:?}");
        assert!(onto_cylinder(v(0.0, 0.0, 0.0), v(0.0, 0.0, 1.0), 9.0, v(0.0, 0.0, 4.0)).is_err());
        assert!(onto_cylinder(v(0.0, 0.0, 0.0), v(0.0, 0.0, 1.0), -1.0, v(1.0, 0.0, 4.0)).is_err());
    }
}
