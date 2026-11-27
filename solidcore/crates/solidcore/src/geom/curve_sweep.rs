//! A curve in space swept along one direction, as the sheet a face is split along (not
//! upstream): the direction to sweep it along (by default the mean normal of the faces at the
//! curve), and each piece of the curve as the exact surface its sweep is: a line a plane strip,
//! an arc about an axis along the sweep a cylinder strip, any other piece (an arc about another
//! axis in rational quadratic pieces of a quarter turn at most) its rational Bézier translated
//! along the sweep, a spline of degree one across it.
//!
//! Every surface here has its normal on the curve's left as the sweep sees it: the sweep
//! direction crossed with the curve's direction of travel. The caller turns them into record
//! surfaces and splits the faces with them.

use crate::geom::vector3d::{Point3, Vec3};
use crate::math::HALF_PI;

/// The sine of the smallest angle the sweep direction may make with a face's tangent plane at
/// the curve for the sweep to cut the face across (1°). At any angle above zero the swept sheet
/// does cross the face, but where the crossing lies is ill-conditioned: an error `e` in the
/// curve or the face, square to the face, moves the crossing by `e / sin(angle)` along the face.
/// The cutoff bounds that growth at `1 / sin 1° ≈ 57`; a shallower sweep at every sample of the
/// curve, down to exactly along the face where the sheet only touches it, is treated as running
/// along the face.
pub const ALONG_FACE: f64 = 0.017_452_406_437_283_512;

/// Why no sweep direction is found for a curve.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SweepRefusal {
    /// No sample of the curve has a foot on any face, and no direction is given.
    NoFace,
    /// The given direction is zero or not finite, or the face normals at the curve cancel out.
    NoDirection,
    /// The direction lies in the face's tangent plane, or within [`ALONG_FACE`] of it, at every
    /// sample of the curve: the sheet would touch the face, or cross it where the crossing moves
    /// more than 57 times any error, for the curve's whole length.
    AlongFace,
}

/// A sample of a curve and the points of the faces nearest it, each with the face's unit normal
/// there (any length is unitized).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sample {
    /// The point of the curve.
    pub at: Point3,
    /// Each face's nearest point and its normal there.
    pub feet: Vec<(Point3, Vec3)>,
}

/// The unit direction to sweep a curve along: `given` (unitized) or, with none, the mean of the
/// face normals at the curve's samples, each at the foot on the face nearest the sample. Refused
/// when no sample has a foot and no direction is given, when there is no direction (`given` zero or not finite, the normals
/// cancel out), or when the direction lies in the face's tangent plane, within [`ALONG_FACE`], at
/// every sample: the swept sheet would touch the face, or cross it so shallowly that where it
/// crosses is not reliable (see [`ALONG_FACE`]).
pub fn sweep_direction(samples: &[Sample], given: Option<Vec3>) -> Result<Vec3, SweepRefusal> {
    let normals: Vec<Vec3> = samples
        .iter()
        .filter_map(|s| {
            s.feet
                .iter()
                .filter(|(p, n)| {
                    p.x.is_finite() && p.y.is_finite() && p.z.is_finite() && n.length().is_finite()
                })
                .min_by(|a, b| s.at.distance(a.0).total_cmp(&s.at.distance(b.0)))
                .and_then(|(_, n)| n.unitize().ok())
        })
        .collect();
    let finite = |v: Vec3| v.x.is_finite() && v.y.is_finite() && v.z.is_finite();
    if normals.is_empty() {
        // No face under the curve to measure against: a given direction stands as it is.
        return match given {
            Some(d) if finite(d) => d.unitize().map_err(|_| SweepRefusal::NoDirection),
            Some(_) => Err(SweepRefusal::NoDirection),
            None => Err(SweepRefusal::NoFace),
        };
    }
    let dir = match given {
        Some(d) if finite(d) => d.unitize().map_err(|_| SweepRefusal::NoDirection)?,
        Some(_) => return Err(SweepRefusal::NoDirection),
        None => normals
            .iter()
            .fold(Vec3::ZERO, |sum, n| sum + *n)
            .unitize()
            .map_err(|_| SweepRefusal::NoDirection)?,
    };
    if normals.iter().all(|n| dir.dot(*n).abs() < ALONG_FACE) {
        return Err(SweepRefusal::AlongFace);
    }
    Ok(dir)
}

/// A line swept along a direction: the plane through `origin` (the line's start) spanned by `u`,
/// the unit sweep direction, and `v`, square to it, the line's direction with its part along `u`
/// taken out: the line runs `length` along `v` (and on along `u`, which the strip, unbounded
/// along `u`, holds). Its normal `u × v` is the line's left.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Strip {
    /// The line's start.
    pub origin: Point3,
    /// The unit sweep direction.
    pub u: Vec3,
    /// The line's unit direction.
    pub v: Vec3,
    /// The line's length.
    pub length: f64,
}

/// The line from `a` to `b` swept along the unit `dir`. `None` for a line of no length, one that
/// runs along `dir` (its sweep has no area), or numbers that are not finite.
#[must_use]
pub fn line_strip(a: Point3, b: Point3, dir: Vec3) -> Option<Strip> {
    let (whole, t) = (b - a).length_and_unit()?;
    if !(whole.is_finite() && a.x.is_finite() && a.y.is_finite() && a.z.is_finite())
        || t.cross(dir).length() <= 1e-9
    {
        return None;
    }
    // Square to the sweep: SMLib's plane holds square axes only.
    let (_, v) = (t - dir * t.dot(dir)).length_and_unit()?;
    let length = (b - a).dot(v);
    (length > 0.0 && length.is_finite()).then_some(Strip {
        origin: a,
        u: dir,
        v,
        length,
    })
}

/// An arc about an axis along the sweep, swept: the cylinder about the frame's origin (the arc's
/// centre), its axis `x × y`, of `radius`, over the angles `angles` from `x` toward `y` (the arc
/// from its start at angle 0). `left_inside` when the curve's left, as the sweep sees it, is
/// inside the cylinder (against its outward normal).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ArcStrip {
    /// The arc's centre.
    pub origin: Point3,
    /// Unit, from the centre to the arc's start.
    pub x: Vec3,
    /// Unit, the axis crossed with `x`.
    pub y: Vec3,
    /// The arc's radius.
    pub radius: f64,
    /// The arc's angles, low then high.
    pub angles: [f64; 2],
    /// The curve's left is inside the cylinder.
    pub left_inside: bool,
}

/// The arc from `start` about `center`, turning `angle` (right hand) about `axis`, swept along
/// the unit `dir`, when its axis runs along `dir` (within 1e-9): a cylinder strip. `None` for an
/// axis across `dir`, an arc of no radius, no turn or more than a turn.
#[must_use]
pub fn arc_strip(
    start: Point3,
    center: Point3,
    axis: Vec3,
    angle: f64,
    dir: Vec3,
) -> Option<ArcStrip> {
    let k = axis.unitize().ok()?;
    let (radius, x) = (start - center).length_and_unit()?;
    if !(radius.is_finite()
        && angle.is_finite()
        && angle != 0.0
        && angle.abs() <= crate::math::TWO_PI + 1e-12)
    {
        return None;
    }
    if k.cross(dir).length() > 1e-9 {
        return None;
    }
    let y = k.cross(x);
    // The direction of travel is (k × x) turned on by the angle's sign; the left, `dir × travel`,
    // is `-x · (dir · k) · sign(angle)`: inside where that is positive.
    let left_inside = dir.dot(k) * angle.signum() > 0.0;
    let angles = if angle > 0.0 {
        [0.0, angle]
    } else {
        [angle, 0.0]
    };
    Some(ArcStrip {
        origin: center,
        x,
        y,
        radius,
        angles,
        left_inside,
    })
}

/// The arc from `start` about `center`, turning `angle` (right hand) about `axis`, as rational
/// quadratic Bézier pieces of a quarter turn at most, each its homogeneous control points
/// `[x·w, y·w, z·w, w]`: the ends on the arc with weight one, the middle point where the end
/// tangents meet with weight `cos(half the piece's turn)`. `None` for no radius, no turn, more
/// than a turn, an axis of no length or numbers that are not finite.
#[must_use]
pub fn arc_beziers(
    start: Point3,
    center: Point3,
    axis: Vec3,
    angle: f64,
) -> Option<Vec<[[f64; 4]; 3]>> {
    let k = axis.unitize().ok()?;
    let (radius, x) = (start - center).length_and_unit()?;
    if !(radius.is_finite()
        && angle.is_finite()
        && angle != 0.0
        && angle.abs() <= crate::math::TWO_PI + 1e-12)
    {
        return None;
    }
    // The arc's own frame: x to the start, y a quarter turn on about k.
    let y = k.cross(x);
    let at = |t: f64, r: f64| center + (x * t.cos() + y * t.sin()) * r;
    let pieces = (angle.abs() / HALF_PI - 1e-9).ceil().max(1.0);
    let h = angle / (2.0 * pieces);
    let w = h.cos();
    let weighted = |p: Point3, w: f64| [p.x * w, p.y * w, p.z * w, w];
    let mut out = Vec::new();
    let mut t0 = 0.0;
    while out.len() < pieces as usize {
        out.push([
            weighted(at(t0, radius), 1.0),
            weighted(at(t0 + h, radius / w), w),
            weighted(at(t0 + 2.0 * h, radius), 1.0),
        ]);
        t0 += 2.0 * h;
    }
    Some(out)
}

/// The rational Bézier over the homogeneous control points `ctrl` (`[x·w, y·w, z·w, w]`, weights
/// positive) translated along the unit `dir` across the box `box_min`..`box_max`: the control
/// rows of the surface of degree one across the sweep, the curve moved to the lowest and to the
/// highest height along `dir` at which some control point passes a corner of the box, so the
/// sheet passes the box every way. `None` for fewer than two control points, a weight that is
/// not positive, numbers that are not finite, or a curve that is a straight line along `dir`
/// (its sweep has no area).
#[must_use]
pub fn extruded_rows(
    ctrl: &[[f64; 4]],
    dir: Vec3,
    box_min: Point3,
    box_max: Point3,
) -> Option<[Vec<[f64; 4]>; 2]> {
    if ctrl.len() < 2
        || ctrl
            .iter()
            .any(|q| q.iter().any(|x| !x.is_finite()) || q[3] <= 0.0)
    {
        return None;
    }
    let points: Vec<Point3> = ctrl
        .iter()
        .map(|q| Vec3::new(q[0] / q[3], q[1] / q[3], q[2] / q[3]))
        .collect();
    let first = *points.first()?;
    let size = points.iter().fold(0.0_f64, |m, p| m.max(p.distance(first)));
    if points
        .iter()
        .all(|p| (*p - first).cross(dir).length() <= 1e-9 * (1.0 + size))
    {
        return None;
    }
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    for k in 0..8_u8 {
        let pick = |bit: u8, a: f64, b: f64| if k & bit == 0 { a } else { b };
        let corner = Vec3::new(
            pick(1, box_min.x, box_max.x),
            pick(2, box_min.y, box_max.y),
            pick(4, box_min.z, box_max.z),
        );
        for p in &points {
            let h = (corner - *p).dot(dir);
            (lo, hi) = (lo.min(h), hi.max(h));
        }
    }
    if !(lo.is_finite() && hi.is_finite() && lo < hi) {
        return None;
    }
    let row = |h: f64| -> Vec<[f64; 4]> {
        ctrl.iter()
            .zip(&points)
            .map(|(q, p)| {
                let m = *p + dir * h;
                [m.x * q[3], m.y * q[3], m.z * q[3], q[3]]
            })
            .collect()
    };
    Some([row(lo), row(hi)])
}

/// How far an open curve's end at `from` must run on in a straight line to pass the box
/// `box_min`..`box_max` from wherever it starts: the box's diagonal and the distance from `from`
/// to its centre.
#[must_use]
pub fn reach_past(box_min: Point3, box_max: Point3, from: Point3) -> f64 {
    (box_max - box_min).length() + from.distance((box_min + box_max) * 0.5)
}

/// The point `far` on from `at` along the unit direction `along` (backward for a negative
/// `far`): where an open curve's end, continued along its tangent, has passed the box.
#[must_use]
pub fn run_on(at: Point3, along: Vec3, far: f64) -> Option<Point3> {
    let t = along.unitize().ok()?;
    let p = at + t * far;
    (p.x.is_finite() && p.y.is_finite() && p.z.is_finite()).then_some(p)
}

/// The point on from `at` along the direction `along` (backward for a negative `far`) that has
/// run `far` square to the sweep `dir`: where an open curve's end, continued along its tangent,
/// has passed the box in the swept sheet (which holds only the run across the sweep). A tangent
/// square to the sweep runs on `far` itself; a steeper one runs on `far / sin(angle to dir)`.
/// `None` for a tangent along `dir` (the sine within 1e-9), a zero vector or numbers that are not
/// finite.
#[must_use]
pub fn run_on_across(at: Point3, along: Vec3, dir: Vec3, far: f64) -> Option<Point3> {
    let t = along.unitize().ok()?;
    let across = t.cross(dir.unitize().ok()?).length();
    if across.is_nan() || across <= 1e-9 {
        return None;
    }
    run_on(at, t, far / across)
}

/// Whether a curve from `start` to `end` closes on itself: its ends within a billionth of its
/// size from the origin, as a sketch chain's.
#[must_use]
pub fn closes(start: Point3, end: Point3) -> bool {
    start.distance(end) <= 1e-9 * (1.0 + start.length())
}

/// How a closed polyline `points` turns about the unit `dir`: its area as seen along `dir`,
/// positive counter-clockwise (right hand about `dir`), where the left of the curve as the sweep
/// sees it is inside.
#[must_use]
pub fn turn_about(points: &[Point3], dir: Vec3) -> f64 {
    let Some(first) = points.first() else {
        return 0.0;
    };
    points
        .windows(2)
        .map(|w| match w {
            [a, b] => (*a - *first).cross(*b - *first).dot(dir),
            _ => 0.0,
        })
        .sum::<f64>()
        * 0.5
}

/// Most points of a closed polyline [`loop_turn`] checks for crossings (pairs of its edges).
const MAX_LOOP_POINTS: usize = 8192;

/// Which way a closed polyline `points` (its last point on its first) turns about the unit `dir`,
/// as seen along `dir`: `Some(true)` counter-clockwise (the left of the curve as the sweep sees
/// it is inside), `Some(false)` clockwise. `None` where that cannot be told: the view crosses
/// itself (its lobes turn opposite ways, so no one side is inside for all of them), it encloses
/// no area (within a billionth of its size squared), it has more than [`MAX_LOOP_POINTS`]
/// points, or its numbers are not finite.
#[must_use]
pub fn loop_turn(points: &[Point3], dir: Vec3) -> Option<bool> {
    let k = dir.unitize().ok()?;
    if points.len() < 4 || points.len() > MAX_LOOP_POINTS {
        return None;
    }
    // Two axes square to the sweep: the view along it.
    let pick = if k.x.abs() < 0.9 { Vec3::X } else { Vec3::Y };
    let (_, u) = (pick - k * pick.dot(k)).length_and_unit()?;
    let v = k.cross(u);
    let first = *points.first()?;
    let flat: Vec<[f64; 2]> = points
        .iter()
        .map(|p| [(*p - first).dot(u), (*p - first).dot(v)])
        .collect();
    if flat.iter().flatten().any(|x| !x.is_finite()) {
        return None;
    }
    let size = flat
        .iter()
        .fold(0.0_f64, |m, [x, y]| m.max(x.abs()).max(y.abs()));
    let area = turn_about(points, k);
    if !(area.is_finite() && area.abs() > 1e-9 * size * size) {
        return None;
    }
    // A crossing: two edges that are not neighbours, each with the other's ends strictly on its
    // two sides (a touch within a billionth of the size counts as none).
    let eps = 1e-9 * size * size;
    let side = |a: [f64; 2], b: [f64; 2], p: [f64; 2]| {
        (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0])
    };
    let apart = |s: f64, t: f64| (s > eps && t < -eps) || (s < -eps && t > eps);
    let edges: Vec<([f64; 2], [f64; 2])> = flat
        .windows(2)
        .filter_map(|w| match w {
            [a, b] => Some((*a, *b)),
            _ => None,
        })
        .collect();
    let last = edges.len().saturating_sub(1);
    for (i, (a, b)) in edges.iter().enumerate() {
        for (j, (c, d)) in edges.iter().enumerate().skip(i + 2) {
            if i == 0 && j == last {
                continue;
            }
            if apart(side(*a, *b, *c), side(*a, *b, *d))
                && apart(side(*c, *d, *a), side(*c, *d, *b))
            {
                return None;
            }
        }
    }
    Some(area > 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::{PI, TWO_PI};

    fn sample(at: Point3, feet: &[(Point3, Vec3)]) -> Sample {
        Sample {
            at,
            feet: feet.to_vec(),
        }
    }

    #[test]
    fn the_direction_is_the_mean_normal_at_the_nearest_feet() {
        // On a cylinder of radius 29 about y at z = -21: normals tilt ±θ about y either side.
        let n = |x: f64| Vec3::new(x / 29.0, 0.0, (29.0_f64 * 29.0 - x * x).sqrt() / 29.0);
        let s: Vec<Sample> = [-20.0, 0.0, 20.0]
            .iter()
            .map(|x| sample(Vec3::new(*x, 0.0, 0.0), &[(Vec3::new(*x, 0.0, 0.0), n(*x))]))
            .collect();
        let d = sweep_direction(&s, None).unwrap();
        assert!((d - Vec3::Z).length() < 1e-15, "{d:?}");
        // Of two faces, the nearer foot decides.
        let two = [sample(
            Vec3::ZERO,
            &[
                (Vec3::new(0.0, 0.0, 5.0), Vec3::X),
                (Vec3::new(0.0, 0.0, 1.0), Vec3::Y * 3.0),
            ],
        )];
        assert_eq!(sweep_direction(&two, None).unwrap(), Vec3::Y);
        // A given direction is unitized.
        assert_eq!(
            sweep_direction(&s, Some(Vec3::new(0.0, 0.0, -4.0))).unwrap(),
            -Vec3::Z
        );
    }

    #[test]
    fn a_sweep_along_the_face_everywhere_is_refused() {
        let flat: Vec<Sample> = (0..5)
            .map(|i| {
                sample(
                    Vec3::new(f64::from(i), 0.0, 0.0),
                    &[(Vec3::new(f64::from(i), 0.0, 0.0), Vec3::Z)],
                )
            })
            .collect();
        assert_eq!(
            sweep_direction(&flat, Some(Vec3::Y)),
            Err(SweepRefusal::AlongFace)
        );
        // Tilted half a degree off the face still runs along it; two degrees cuts across.
        let tilt = |deg: f64| Vec3::new(0.0, deg.to_radians().cos(), deg.to_radians().sin());
        assert_eq!(
            sweep_direction(&flat, Some(tilt(0.5))),
            Err(SweepRefusal::AlongFace)
        );
        assert!(sweep_direction(&flat, Some(tilt(2.0))).is_ok());
        assert_eq!(
            sweep_direction(&flat, Some(Vec3::ZERO)),
            Err(SweepRefusal::NoDirection)
        );
        assert_eq!(
            sweep_direction(&flat, Some(Vec3::new(f64::NAN, 0.0, 1.0))),
            Err(SweepRefusal::NoDirection)
        );
        assert_eq!(
            sweep_direction(&[sample(Vec3::ZERO, &[])], None),
            Err(SweepRefusal::NoFace)
        );
        // With no face under the curve a given direction stands.
        assert_eq!(
            sweep_direction(&[sample(Vec3::ZERO, &[])], Some(Vec3::Y * 2.0)),
            Ok(Vec3::Y)
        );
        // Normals that cancel out give no direction.
        let both = [
            sample(Vec3::ZERO, &[(Vec3::ZERO, Vec3::Z)]),
            sample(Vec3::X, &[(Vec3::X, -Vec3::Z)]),
        ];
        assert_eq!(sweep_direction(&both, None), Err(SweepRefusal::NoDirection));
    }

    #[test]
    fn the_cutoff_is_where_a_crossing_moves_57_times_an_error() {
        // 1 / sin 1° = 57.30: an error e square to the face moves the crossing by e / sin(angle).
        assert!((1.0 / ALONG_FACE - 57.298_688_498_550_18).abs() < 1e-9);
        assert!((ALONG_FACE - 1.0_f64.to_radians().sin()).abs() < 1e-17);
        let flat = [sample(Vec3::ZERO, &[(Vec3::ZERO, Vec3::Z)])];
        let tilt = |deg: f64| Vec3::new(0.0, deg.to_radians().cos(), deg.to_radians().sin());
        assert_eq!(
            sweep_direction(&flat, Some(tilt(0.99))),
            Err(SweepRefusal::AlongFace)
        );
        assert!(sweep_direction(&flat, Some(tilt(1.01))).is_ok());
    }

    #[test]
    fn a_line_sweeps_to_a_plane_strip_on_its_left() {
        let s = line_strip(Vec3::new(1.0, 2.0, 0.0), Vec3::new(1.0, 12.0, 0.0), Vec3::Z).unwrap();
        assert_eq!(
            (s.origin, s.u, s.v, s.length),
            (Vec3::new(1.0, 2.0, 0.0), Vec3::Z, Vec3::Y, 10.0)
        );
        // u × v = z × y = -x: the left of a line running +y, seen from +z.
        assert_eq!(s.u.cross(s.v), -Vec3::X);
        // A line slanting along the sweep: v square to it, the line's run across the sweep.
        let slant = line_strip(Vec3::ZERO, Vec3::new(10.0, 0.0, 10.0), Vec3::Z).unwrap();
        assert_eq!((slant.v, slant.length), (Vec3::X, 10.0));
        assert!(line_strip(Vec3::ZERO, Vec3::Z * 3.0, Vec3::Z).is_none());
        assert!(line_strip(Vec3::ZERO, Vec3::ZERO, Vec3::Z).is_none());
    }

    #[test]
    fn an_arc_about_the_sweep_is_a_cylinder_strip() {
        // A quarter turn of radius 5 about +z from (5, 0, 0).
        let a = arc_strip(
            Vec3::new(5.0, 0.0, 2.0),
            Vec3::new(0.0, 0.0, 2.0),
            Vec3::Z,
            HALF_PI,
            Vec3::Z,
        )
        .unwrap();
        assert_eq!(
            (a.x, a.y, a.radius, a.angles),
            (Vec3::X, Vec3::Y, 5.0, [0.0, HALF_PI])
        );
        assert!(
            a.left_inside,
            "counter-clockwise seen from the sweep: the left is inside"
        );
        assert!(
            !arc_strip(
                Vec3::new(5.0, 0.0, 2.0),
                Vec3::new(0.0, 0.0, 2.0),
                Vec3::Z,
                HALF_PI,
                -Vec3::Z
            )
            .unwrap()
            .left_inside
        );
        assert!(
            !arc_strip(
                Vec3::new(5.0, 0.0, 2.0),
                Vec3::new(0.0, 0.0, 2.0),
                Vec3::Z,
                -HALF_PI,
                Vec3::Z
            )
            .unwrap()
            .left_inside
        );
        assert!(arc_strip(
            Vec3::new(5.0, 0.0, 2.0),
            Vec3::new(0.0, 0.0, 2.0),
            Vec3::Z,
            HALF_PI,
            Vec3::X
        )
        .is_none());
        assert!(arc_strip(Vec3::ZERO, Vec3::ZERO, Vec3::Z, HALF_PI, Vec3::Z).is_none());
    }

    #[test]
    fn an_arc_is_quarter_turn_rational_quadratics_on_its_circle() {
        let (c, r) = (Vec3::new(1.0, 2.0, 3.0), 4.0);
        let k = Vec3::new(0.0, 1.0, 1.0);
        let start = c + Vec3::X * r;
        let pieces = arc_beziers(start, c, k, 1.5 * PI).unwrap();
        assert_eq!(pieces.len(), 3);
        let point = |p: &[[f64; 4]; 3], t: f64| {
            let b = [(1.0 - t) * (1.0 - t), 2.0 * t * (1.0 - t), t * t];
            let h = (0..4)
                .map(|i| p.iter().zip(&b).map(|(q, b)| q[i] * b).sum::<f64>())
                .collect::<Vec<_>>();
            Vec3::new(h[0] / h[3], h[1] / h[3], h[2] / h[3])
        };
        let ku = k.unitize().unwrap();
        for p in &pieces {
            for t in [0.0, 0.3, 0.5, 0.9, 1.0] {
                let q = point(p, t);
                assert!((q.distance(c) - r).abs() < 1e-12, "off the circle at {t}");
                assert!((q - c).dot(ku).abs() < 1e-12, "off the arc's plane");
            }
        }
        // Ends: the start, then three quarter turns on about k (right hand).
        let end = point(pieces.last().unwrap(), 1.0);
        let want = c + (Vec3::X * (1.5 * PI).cos() + ku.cross(Vec3::X) * (1.5 * PI).sin()) * r;
        assert!(end.distance(want) < 1e-12);
        assert!(point(&pieces[0], 0.0).distance(start) < 1e-12);
        assert!(arc_beziers(start, c, k, TWO_PI + 0.1).is_none());
        assert!(arc_beziers(start, c, k, 0.0).is_none());
    }

    #[test]
    fn a_bezier_extrudes_past_the_box() {
        let ctrl = [
            [0.0, 0.0, 0.0, 1.0],
            [5.0, 10.0, 0.0, 1.0],
            [10.0, 0.0, 0.0, 1.0],
        ];
        let [lo, hi] = extruded_rows(
            &ctrl,
            Vec3::Z,
            Vec3::new(-1.0, -1.0, -3.0),
            Vec3::new(11.0, 11.0, 7.0),
        )
        .unwrap();
        assert_eq!(
            lo,
            vec![
                [0.0, 0.0, -3.0, 1.0],
                [5.0, 10.0, -3.0, 1.0],
                [10.0, 0.0, -3.0, 1.0]
            ]
        );
        assert_eq!(
            hi,
            vec![
                [0.0, 0.0, 7.0, 1.0],
                [5.0, 10.0, 7.0, 1.0],
                [10.0, 0.0, 7.0, 1.0]
            ]
        );
        // Weights stay with their points.
        let rational = [
            [0.0, 0.0, 0.0, 1.0],
            [2.5, 0.0, 0.0, 0.5],
            [10.0, 0.0, 0.0, 1.0],
        ];
        let [lo, _] = extruded_rows(
            &rational,
            Vec3::Y,
            Vec3::new(0.0, -2.0, 0.0),
            Vec3::new(10.0, 2.0, 0.0),
        )
        .unwrap();
        assert_eq!(lo[1], [2.5, -1.0, 0.0, 0.5]);
        // A straight curve along the sweep has no sheet.
        assert!(extruded_rows(
            &[[0.0, 0.0, 0.0, 1.0], [0.0, 0.0, 4.0, 1.0]],
            Vec3::Z,
            Vec3::ZERO,
            Vec3::Z
        )
        .is_none());
        assert!(extruded_rows(
            &[[0.0, 0.0, 0.0, 0.0], [1.0, 0.0, 0.0, 1.0]],
            Vec3::Z,
            Vec3::ZERO,
            Vec3::Z
        )
        .is_none());
    }

    #[test]
    fn ends_closes_and_turns() {
        assert_eq!(
            reach_past(
                Vec3::ZERO,
                Vec3::new(3.0, 4.0, 0.0),
                Vec3::new(1.5, 2.0, 0.0)
            ),
            5.0
        );
        assert_eq!(
            run_on(Vec3::X, Vec3::Y * 2.0, -3.0),
            Some(Vec3::new(1.0, -3.0, 0.0))
        );
        // Square to the sweep: `far` itself. At 45° to it: far / sin 45° = 2√2 along (1, 0, 1)/√2,
        // which is 2 across. Along the sweep: none.
        assert_eq!(
            run_on_across(Vec3::X, Vec3::Y * 2.0, Vec3::Z, -3.0),
            Some(Vec3::new(1.0, -3.0, 0.0))
        );
        let slant = run_on_across(Vec3::ZERO, Vec3::new(1.0, 0.0, 1.0), Vec3::Z, 2.0).unwrap();
        assert!(
            slant.distance(Vec3::new(2.0, 0.0, 2.0)) < 1e-12,
            "{slant:?}"
        );
        assert_eq!(
            run_on_across(Vec3::ZERO, Vec3::Z, -Vec3::Z * 3.0, 2.0),
            None
        );
        assert!(closes(Vec3::X, Vec3::X) && !closes(Vec3::X, Vec3::Y));
        let square = [
            Vec3::ZERO,
            Vec3::X,
            Vec3::new(1.0, 1.0, 0.0),
            Vec3::Y,
            Vec3::ZERO,
        ];
        assert_eq!(turn_about(&square, Vec3::Z), 1.0);
        assert_eq!(turn_about(&square, -Vec3::Z), -1.0);
    }

    #[test]
    fn a_loop_turns_one_way_unless_it_crosses_itself_or_has_no_area() {
        let square = [
            Vec3::ZERO,
            Vec3::X,
            Vec3::new(1.0, 1.0, 0.0),
            Vec3::Y,
            Vec3::ZERO,
        ];
        assert_eq!(loop_turn(&square, Vec3::Z), Some(true));
        assert_eq!(loop_turn(&square, -Vec3::Z), Some(false));
        let reversed: Vec<Vec3> = square.iter().rev().copied().collect();
        assert_eq!(loop_turn(&reversed, Vec3::Z), Some(false));
        // A circle of radius 0.005 in 8 points either way round.
        let circle = |sign: f64| -> Vec<Vec3> {
            (0..=8)
                .map(|i| {
                    let t = sign * TWO_PI * f64::from(i) / 8.0;
                    Vec3::new(0.005 * t.cos(), 0.005 * t.sin(), 0.0)
                })
                .collect()
        };
        assert_eq!(loop_turn(&circle(1.0), Vec3::Z), Some(true));
        assert_eq!(loop_turn(&circle(-1.0), Vec3::Z), Some(false));
        // A chord there and back encloses nothing: not told, not clockwise.
        let chord = [Vec3::ZERO, Vec3::X * 0.01, Vec3::ZERO];
        assert_eq!(loop_turn(&chord, Vec3::Z), None);
        // (0,0,0), (4,4,0), (0,4,2), (4,0,2) seen along +z: a bow tie whose lobes enclose +8 and
        // -8 (the total turn is 0); either way, its lobes turn opposite ways.
        let bow = [
            Vec3::ZERO,
            Vec3::new(4.0, 4.0, 0.0),
            Vec3::new(0.0, 4.0, 2.0),
            Vec3::new(4.0, 0.0, 2.0),
            Vec3::ZERO,
        ];
        assert_eq!(turn_about(&bow, Vec3::Z), 0.0);
        assert_eq!(loop_turn(&bow, Vec3::Z), None);
        // Lobes of unequal size, (0,0), (4,4), (0,4), (6,0): the total turn (16 − 24) / 2 = -4
        // favours one lobe, and the view still crosses itself.
        let uneven = [
            Vec3::ZERO,
            Vec3::new(4.0, 4.0, 0.0),
            Vec3::new(0.0, 4.0, 0.0),
            Vec3::new(6.0, 0.0, 0.0),
            Vec3::ZERO,
        ];
        assert_eq!(turn_about(&uneven, Vec3::Z), -4.0);
        assert_eq!(loop_turn(&uneven, Vec3::Z), None);
        assert_eq!(loop_turn(&square, Vec3::ZERO), None);
    }
}
