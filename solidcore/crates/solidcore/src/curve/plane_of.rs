//! The plane a set of curves lies in, within a tolerance, certified.
//!
//! Upstream: none (not upstream; solidcore-native).
//!
//! | Rust          | Upstream |
//! |---------------|----------|
//! | [`plane_of`]  | none     |
//! | [`PlaneOf`]   | none     |
//!
//! Each curve lies in the convex hull of points the record gives it: a line segment of its two
//! points, a polyline and a spline of their control points, a rational spline of its Euclidean
//! control points (weights positive), a circle or an ellipse of the four corners of the
//! parallelogram `origin ± r_x x ± r_y y` round it (any arc of it lies inside that too).
//! A slab is convex, so every point of every curve is within the tolerance of a plane when
//! every one of those points is. The plane is the one `surface::implicit::plane_of_net` takes
//! for a net: through the first point, square to the two chords from there that span the most
//! (the farthest point, then the one farthest off that chord), so its normal is as well
//! conditioned as the points allow.
//!
//! Rounding (docs/definitions-and-certificates.md §4.4): a corner of a conic carries
//! `γ_3 (|o| + r_x |x| + r_y |y|)` per coordinate, a Euclidean point of a rational spline
//! `γ_1` of itself; a point off by `e` per coordinate is at most `2 e` (`√3 e`) farther from the
//! plane. The distance `|(q - c) . n| / |n|` is computed with the normal made unit: the
//! difference rounds by `γ_1` and the dot by `γ_3` of `|q - c|` per component (`2 (γ_1 + γ_3)
//! |q - c|` in all), and the unit normal's length is 1 within `γ_8`.
//!
//! Deviations: none (no upstream counterpart). A line is read as its segment from `start` to
//! `end`. No recursion.

use crate::error::{Result, SolidError};
use crate::eval::Curve;
use crate::math::{Point3, Vec3};
use crate::nurbs::math::gamma;

const OP: &str = "curve::plane_of::plane_of";

/// A plane the curves lie in: a point of it, its unit normal, and the certified bound of the
/// largest distance of any point of the curves from it (at most the tolerance asked).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlaneOf {
    /// A point of the plane: the curves' first point.
    pub origin: Point3,
    /// The plane's unit normal.
    pub normal: Vec3,
    /// No point of the curves is farther from the plane than this.
    pub deviation: f64,
}

/// The plane every point of `curves` lies within `tol` of, certified (module docs), or `None`
/// when no such plane is proved: the curves are not planar within `tol`, or they span no plane
/// (all within `tol` of one point or of one line).
///
/// # Errors
///
/// `InvalidInput` for no curves, a tolerance not finite and positive, numbers not finite, or
/// a rational spline with a weight not positive; `Unsupported` for a traced intersection
/// curve; a query failure for curves so large their chords or normal overflow.
pub fn plane_of(curves: &[Curve], tol: f64) -> Result<Option<PlaneOf>> {
    if curves.is_empty() {
        return Err(SolidError::invalid_input(OP, "no curves"));
    }
    if !(tol.is_finite() && tol > 0.0) {
        return Err(SolidError::invalid_input(
            OP,
            "a tolerance not finite and positive",
        ));
    }
    let mut points: Vec<(Point3, f64)> = Vec::new();
    for c in curves {
        hull(c, &mut points)?;
    }
    if !points
        .iter()
        .all(|(p, e)| p.x.is_finite() && p.y.is_finite() && p.z.is_finite() && e.is_finite())
    {
        return Err(SolidError::invalid_input(OP, "numbers not finite"));
    }
    let Some(&(c0, e0)) = points.first() else {
        return Ok(None);
    };
    let far = |a: &&(Point3, f64), b: &&(Point3, f64)| {
        (a.0 - c0).length().total_cmp(&(b.0 - c0).length())
    };
    let Some(&(along, _)) = points.iter().max_by(far) else {
        return Ok(None);
    };
    let chord = along - c0;
    let reach = chord.length();
    if reach <= tol {
        return Ok(None);
    }
    let off = |a: &&(Point3, f64), b: &&(Point3, f64)| {
        chord
            .cross(a.0 - c0)
            .length()
            .total_cmp(&chord.cross(b.0 - c0).length())
    };
    let Some(&(across, _)) = points.iter().max_by(off) else {
        return Ok(None);
    };
    let cross = chord.cross(across - c0);
    // every point within tol of the line along the chord: no plane is singled out
    if cross.length() / reach <= tol {
        return Ok(None);
    }
    let Ok(normal) = cross.unitize() else {
        return Ok(None);
    };
    // A size past the floating range (a chord or a cross product overflowing) leaves no
    // normal to bound with: refused, never read as a plane. `f64::max` would drop a NaN.
    let overflow = || SolidError::query_failed(OP, "the curves' size overflows the bound");
    if !(reach.is_finite() && normal.x.is_finite() && normal.y.is_finite() && normal.z.is_finite())
    {
        return Err(overflow());
    }
    let mut deviation = 0.0_f64;
    for &(q, e) in &points {
        let w = q - c0;
        let d = w.dot(normal).abs();
        let bound = (d * (1.0 + gamma(8))
            + 2.0 * (gamma(1) + gamma(3)) * w.length() * (1.0 + gamma(5))
            + 2.0 * (e + e0))
            * (1.0 + gamma(2));
        if !bound.is_finite() {
            return Err(overflow());
        }
        deviation = deviation.max(bound);
    }
    Ok((deviation <= tol).then_some(PlaneOf {
        origin: c0,
        normal,
        deviation,
    }))
}

/// Pushes the points whose convex hull holds `c`, each with the error of its coordinates.
fn hull(c: &Curve, out: &mut Vec<(Point3, f64)>) -> Result<()> {
    let p3 = |p: &[f64; 3]| Point3::new(p[0], p[1], p[2]);
    match c {
        Curve::Line { start, end } => {
            out.push((p3(start), 0.0));
            out.push((p3(end), 0.0));
        }
        Curve::Polyline { points } | Curve::Spline { points, .. } => {
            out.extend(points.iter().map(|p| (p3(p), 0.0)));
        }
        Curve::RationalSpline { homogeneous, .. } => {
            for &[x, y, z, w] in homogeneous {
                if !(w.is_finite() && w > 0.0) {
                    return Err(SolidError::invalid_input(OP, "a weight not positive"));
                }
                let q = Point3::new(x / w, y / w, z / w);
                let e = gamma(1) * q.x.abs().max(q.y.abs()).max(q.z.abs());
                out.push((q, e));
            }
        }
        Curve::Circle { frame, radius } => corners(frame, *radius, *radius, out),
        Curve::Ellipse { frame, radii } => corners(frame, radii[0], radii[1], out),
        Curve::SurfaceIntersection { .. } => {
            return Err(SolidError::Unsupported {
                operation: OP,
                reason: "a traced intersection curve",
            })
        }
    }
    Ok(())
}

/// The four corners of the parallelogram `origin ± rx x ± ry y` round a conic of the frame,
/// each with the rounding of computing it.
fn corners(frame: &crate::eval::Frame, rx: f64, ry: f64, out: &mut Vec<(Point3, f64)>) {
    let v = |p: [f64; 3]| Vec3::new(p[0], p[1], p[2]);
    let (o, x, y) = (v(frame.origin), v(frame.x_axis), v(frame.y_axis));
    let (rx, ry) = (rx.abs(), ry.abs());
    let size = |a: Vec3| a.x.abs().max(a.y.abs()).max(a.z.abs());
    let e = gamma(3) * (size(o) + rx * size(x) + ry * size(y));
    for (sx, sy) in [(1.0, 1.0), (1.0, -1.0), (-1.0, 1.0), (-1.0, -1.0)] {
        out.push((o + x * (sx * rx) + y * (sy * ry), e));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::Frame;

    fn spline(points: &[[f64; 3]]) -> Curve {
        let n = points.len();
        let mut knots = vec![0.0; 4];
        knots.extend((1..n - 3).map(|i| i as f64 / (n - 3) as f64));
        knots.extend([1.0; 4]);
        Curve::Spline {
            knots,
            points: points.to_vec(),
        }
    }

    #[test]
    fn a_spline_and_a_line_in_one_plane_give_it() {
        let wave = spline(&[
            [0.0, 0.0, 5.0],
            [10.0, 4.0, 5.0],
            [20.0, -3.0, 5.0],
            [30.0, 2.0, 5.0],
        ]);
        let line = Curve::Line {
            start: [30.0, 2.0, 5.0],
            end: [0.0, 0.0, 5.0],
        };
        let plane = plane_of(&[wave, line], 1e-6).unwrap().unwrap();
        assert_eq!(plane.origin, Point3::new(0.0, 0.0, 5.0));
        assert!(plane.normal.z.abs() > 1.0 - 1e-15, "{:?}", plane.normal);
        assert!(plane.deviation < 1e-12, "{}", plane.deviation);
    }

    #[test]
    fn a_twisted_spline_is_no_plane_unless_the_tolerance_takes_it() {
        let twisted = spline(&[
            [0.0, 0.0, 0.0],
            [10.0, 0.0, 0.0],
            [10.0, 10.0, 0.0],
            [0.0, 10.0, 1.0],
        ]);
        assert_eq!(
            plane_of(std::slice::from_ref(&twisted), 1e-6).unwrap(),
            None
        );
        let slightly = spline(&[
            [0.0, 0.0, 0.0],
            [10.0, 0.0, 0.0],
            [10.0, 10.0, 0.0],
            [0.0, 10.0, 1e-8],
        ]);
        let plane = plane_of(&[slightly], 1e-6).unwrap().unwrap();
        assert!(plane.deviation <= 1e-6 && plane.deviation >= 0.0);
        assert!(plane_of(&[twisted], 2.0).unwrap().is_some());
    }

    #[test]
    fn collinear_curves_or_one_point_span_no_plane() {
        let a = Curve::Line {
            start: [0.0, 0.0, 0.0],
            end: [1.0, 1.0, 1.0],
        };
        let b = Curve::Polyline {
            points: vec![[2.0, 2.0, 2.0], [5.0, 5.0, 5.0]],
        };
        assert_eq!(plane_of(&[a, b], 1e-6).unwrap(), None);
        let dot = Curve::Line {
            start: [1.0, 2.0, 3.0],
            end: [1.0, 2.0, 3.0],
        };
        assert_eq!(plane_of(&[dot], 1e-6).unwrap(), None);
    }

    #[test]
    fn a_circle_lies_in_its_frames_plane() {
        let s = std::f64::consts::FRAC_1_SQRT_2;
        let frame = Frame {
            origin: [1.0, 2.0, 3.0],
            x_axis: [s, s, 0.0],
            y_axis: [-0.5, 0.5, s],
        };
        let circle = Curve::Circle { frame, radius: 4.0 };
        let plane = plane_of(&[circle], 1e-9).unwrap().unwrap();
        // the frame's z: x cross y
        let z = Vec3::new(s * s, -s * s, s * 0.5 + s * 0.5);
        let along = plane.normal.dot(z).abs() / z.length();
        assert!((along - 1.0).abs() < 1e-12, "{:?}", plane.normal);
    }

    #[test]
    fn curves_too_large_for_the_bound_are_refused_not_read_as_a_plane() {
        // finite points whose chords' cross product overflows: no finite normal
        let huge = Curve::Polyline {
            points: vec![[0.0; 3], [1e200, 0.0, 0.0], [0.0, 1e200, 0.0]],
        };
        assert!(matches!(
            plane_of(&[huge], 1e-6),
            Err(SolidError::Failed(_))
        ));
    }

    #[test]
    fn bad_input_is_refused() {
        assert!(plane_of(&[], 1e-6).is_err());
        let line = Curve::Line {
            start: [0.0; 3],
            end: [1.0, 0.0, 0.0],
        };
        assert!(plane_of(std::slice::from_ref(&line), 0.0).is_err());
        assert!(plane_of(&[line], f64::NAN).is_err());
        let bad = Curve::RationalSpline {
            knots: vec![0.0, 0.0, 1.0, 1.0],
            homogeneous: vec![[0.0, 0.0, 0.0, 1.0], [1.0, 0.0, 0.0, -1.0]],
        };
        assert!(plane_of(&[bad], 1e-6).is_err());
    }
}
