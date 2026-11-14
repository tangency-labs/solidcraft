//! (not upstream) Whether one curve stays within a distance of another over an interval of
//! each, certified: an enclosure of the two curves as defined, rounding included, never
//! samples.
//!
//! Upstream: none. `SmTools::FindCoincidentEdge` ([`crate::geom::tools::find_coincident_edge`])
//! samples the distance at 30 points; a decision that two curves are one edge needs every
//! point of each within the tolerance of the other (docs/booleans.md, "One edge or two").
//!
//! | Rust                       | Kind                                                    |
//! |----------------------------|---------------------------------------------------------|
//! | [`Curve::proved_within`]   | a line, a B-spline, and a conic by its rational B-spline |
//! | [`Curve::proved_together`] | both ways                                               |
//!
//! **A tube.** A piece of a curve is enclosed by its chord and a radius `rho`: every point
//! of the piece lies within `rho` of the chord's segment. A line's piece is its segment,
//! `rho` the rounding of its ends ([`Line::segment_over`](crate::curve::line::Line::segment_over)).
//! A B-spline's piece within a knot span lies in the convex hull of its Bezier control
//! points (positive weights; Piegl and Tiller, *The NURBS Book*, 2nd ed., §4.4), read off
//! the span by blossoming on the piece's own interval with a derived rounding bound
//! ([`Span::hull`]); the distance to a segment is convex, so `rho` is the largest distance
//! of a control point from the chord, plus the points' rounding. A piece over several knot
//! spans takes the points of each span's part against the one chord between its ends.
//!
//! The converse holds too: the piece is a continuous path between the chord's ends (to
//! their rounding `e`) within `rho` of the chord, so its projection onto the chord's line
//! covers the chord but for `e` at each end, and the point of the piece above a point of
//! the chord is within `rho` of it. Every point of the chord is within `rho` of the piece,
//! `rho` taken at least `2e`.
//!
//! **The bound.** For a piece of `A` with tube `(S_A, rho_A)` and a piece of `B` with tube
//! `(S_B, rho_B)`: a point of the piece of `A` is within `rho_A` of `S_A`; a point of `S_A`
//! is no farther from `S_B` than the farther of its two ends (convexity again); a point of
//! `S_B` is within `rho_B` of the piece of `B`. So the piece of `A` lies within
//! `rho_A + max(d(a_0, S_B), d(a_1, S_B)) + rho_B` of `B`, each distance an upper bound
//! (the foot's parameter is clamped to the segment, so its rounding only moves the foot
//! along the segment; the difference and the norm round by `gamma_4`).
//!
//! **The search.** `A`'s interval is cut at its knots, and each piece is tried against the
//! piece of `B` between the feet of its two ends (`B`'s own point drop: only a guess of
//! where to look, any piece of `B` inside its interval gives a true bound). A piece whose
//! bound exceeds the radius is halved, at most [`DEPTH`] times and [`MAX_PIECES`] pieces
//! in all; past that nothing is proved. A tube's radius falls with the square of its
//! length (the sagitta), so pieces whose curves run `d` apart are proved within any radius
//! above `d` once they are about `sqrt(8 R (radius - d))` long, `R` their radius of
//! curvature.
//!
//! Deviations: not a port. Curves of other kinds (offset, composite, iso, projected and
//! curve-on-surface curves, a Hermite curve, a conic without its B-spline) have no hull
//! here, and nothing is proved for them. No recursion: the pieces wait on a stack bounded
//! by [`MAX_PIECES`].

use crate::curve::curve::{Curve, CurveKind};
use crate::curve::length_bound::Span;
use crate::error::Result;
use crate::geom::extent1d::Extent1d;
use crate::geom::global_solver::SolverOperation;
use crate::geom::vector3d::Vec3;
use crate::kernel::Kernel;
use crate::math::EFF_ZERO;
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::math::gamma;

const OP: &str = "curve::proximity::proved_within";

/// How many times a piece is halved at most.
pub const DEPTH: usize = 32;

/// The most pieces tried for one bound.
pub const MAX_PIECES: usize = 4096;

/// A piece of a curve enclosed by a segment: the piece lies within `rho` of the segment
/// from `a` to `b`, and the segment within `rho` of the piece (module docs).
#[derive(Clone, Copy, Debug)]
struct Tube {
    a: Vec3,
    b: Vec3,
    rho: f64,
}

/// The form a curve is enclosed by.
enum Form<'c> {
    Line(&'c crate::curve::line::Line),
    Spline(&'c BSplineCurve),
}

/// The enclosing form of `curve`: `None` for a kind without one (module docs).
fn form(curve: &Curve) -> Option<Form<'_>> {
    let spline = match curve.kind() {
        CurveKind::Line(l) => return Some(Form::Line(l)),
        CurveKind::BSpline(c) => c,
        CurveKind::Ellipse(c) => c.base(),
        CurveKind::Hyperbola(c) => c.base(),
        CurveKind::Parabola(c) => c.base(),
        _ => return None,
    };
    spline.nurb.is_some().then_some(Form::Spline(spline))
}

/// A failed evaluation proves nothing (`None`); a cancellation and an unported callee are
/// passed on.
fn soft<T>(r: Result<T>) -> Result<Option<T>> {
    match r {
        Ok(v) => Ok(Some(v)),
        Err(e) if e.is_unported_or_cancelled() => Err(e),
        Err(_) => Ok(None),
    }
}

/// An upper bound of the distance from `p` to the segment from `a` to `b` (module docs).
fn segment_distance(p: Vec3, a: Vec3, b: Vec3) -> f64 {
    let along = b - a;
    let w = p - a;
    let len2 = along.dot(along);
    let t = if len2 > 0.0 {
        (w.dot(along) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    ((w - along * t).length() + gamma(4) * (w.length() + along.length())) * (1.0 + gamma(2))
}

/// `a`, and the unique knots of `c` strictly inside `(a, b)`, and `b`.
fn spline_breaks(c: &BSplineCurve, a: f64, b: f64) -> Result<Vec<f64>> {
    let (unique, _) = c.knots(None)?;
    let mut out = vec![a];
    out.extend(unique.into_iter().filter(|&t| t > a && t < b));
    out.push(b);
    Ok(out)
}

impl Form<'_> {
    /// The parameters to cut `[a, b]` at so that each piece lies in one knot span.
    fn breaks(&self, a: f64, b: f64) -> Result<Vec<f64>> {
        match self {
            Form::Line(_) => Ok(vec![a, b]),
            Form::Spline(c) => spline_breaks(c, a, b),
        }
    }

    /// The tube of the piece over `[a, b]` (`a <= b`); `None` where it has no certified
    /// hull (a weight not positive, a number not finite, a malformed B-spline).
    fn tube(&self, a: f64, b: f64) -> Result<Option<Tube>> {
        let sqrt3 = 3.0_f64.sqrt();
        match self {
            Form::Line(l) => {
                let Some([(p, ep), (q, eq)]) = l.segment_over(&Extent1d::from_min_max(a, b)) else {
                    return Ok(None);
                };
                Ok(Some(Tube {
                    a: p,
                    b: q,
                    rho: 2.0 * sqrt3 * ep.max(eq),
                }))
            }
            Form::Spline(c) => {
                let cuts = spline_breaks(c, a, b)?;
                let mut points: Vec<Vec3> = Vec::new();
                let mut rounding = 0.0_f64;
                for w in cuts.windows(2) {
                    let [x, y] = *w else { continue };
                    let hull = if y > x {
                        soft(Span::of(c, 0.5 * (x + y)))?.and_then(|span| span.hull(x, y))
                    } else {
                        // a piece of no length: its point as evaluated
                        let p = c.evaluate_point(x)?;
                        let size = p.x.abs().max(p.y.abs()).max(p.z.abs());
                        let e = gamma(16 * (c.degree()? + 2)) * (1.0 + size);
                        Some((vec![p], e))
                    };
                    let Some((pts, e)) = hull else {
                        return Ok(None);
                    };
                    points.extend(pts);
                    rounding = rounding.max(e);
                }
                let (Some(&first), Some(&last)) = (points.first(), points.last()) else {
                    return Ok(None);
                };
                let reach = points
                    .iter()
                    .fold(0.0_f64, |m, &p| m.max(segment_distance(p, first, last)));
                let rho = reach + 2.0 * sqrt3 * rounding;
                Ok(rho.is_finite().then_some(Tube {
                    a: first,
                    b: last,
                    rho,
                }))
            }
        }
    }
}

impl Curve {
    /// Whether every point of this curve over `interval` is proved to lie within `radius`
    /// of `other` over `other_interval`, or within one of the balls `ends` (a centre and a
    /// radius each: the vertices `other` runs between, with their tolerances, which hold
    /// its ends) (module docs). `false` proves nothing: the curves may be farther apart, of
    /// a kind without a hull, or closer than the pieces tried could show.
    ///
    /// # Errors
    ///
    /// The kernel's cancellation (a failed evaluation or point drop proves nothing).
    pub fn proved_within(
        &self,
        k: &Kernel,
        interval: &Extent1d,
        other: &Self,
        other_interval: &Extent1d,
        radius: f64,
        ends: &[(Vec3, f64)],
    ) -> Result<bool> {
        let (a, b) = (interval.min(), interval.max());
        let (lo, hi) = (other_interval.min(), other_interval.max());
        let finite = [a, b, lo, hi, radius].iter().all(|v| v.is_finite());
        if !(finite && a <= b && lo <= hi && radius >= 0.0) {
            return Ok(false);
        }
        let (Some(mine), Some(theirs)) = (form(self), form(other)) else {
            return Ok(false);
        };
        let Some(cuts) = soft(mine.breaks(a, b))? else {
            return Ok(false);
        };
        let mut stack: Vec<(f64, f64, usize)> = cuts
            .windows(2)
            .filter_map(|w| match *w {
                [x, y] => Some((x, y, DEPTH)),
                _ => None,
            })
            .collect();
        // the foot of a point on the other curve: where to look, not part of the proof (the
        // drop snaps a foot to an end of the interval only within the scaled zero)
        let foot = |p: Vec3| -> Result<Option<f64>> {
            let zero = EFF_ZERO * (1.0 + p.x.abs().max(p.y.abs()).max(p.z.abs()));
            let dropped = soft(other.drop_point(
                k,
                other_interval,
                p,
                None,
                zero,
                None,
                SolverOperation::Minimize,
            ))?;
            Ok(dropped
                .filter(|d| d.success && d.parameter.is_finite())
                .map(|d| d.parameter.max(lo).min(hi)))
        };
        let mut tried = 0_usize;
        while let Some((x, y, depth)) = stack.pop() {
            k.check_cancelled(OP)?;
            tried += 1;
            if tried > MAX_PIECES {
                return Ok(false);
            }
            let Some(piece) = soft(mine.tube(x, y))?.flatten() else {
                return Ok(false);
            };
            // in a ball: the tube's segment is no farther from the centre than its ends
            let in_ball = |&(centre, reach): &(Vec3, f64)| {
                let far = (piece.a - centre).length().max((piece.b - centre).length());
                let far = far * (1.0 + gamma(4)) + gamma(4) * centre.length();
                (piece.rho + far) * (1.0 + gamma(1)) <= reach
            };
            if ends.iter().any(in_ball) {
                continue;
            }
            let (Some(s), Some(t)) = (foot(piece.a)?, foot(piece.b)?) else {
                return Ok(false);
            };
            let Some(facing) = soft(theirs.tube(s.min(t), s.max(t)))?.flatten() else {
                return Ok(false);
            };
            let reach = segment_distance(piece.a, facing.a, facing.b)
                .max(segment_distance(piece.b, facing.a, facing.b));
            let bound = (piece.rho + reach + facing.rho) * (1.0 + gamma(3));
            if bound <= radius {
                continue;
            }
            let mid = 0.5 * (x + y);
            if depth == 0 || !(x < mid && mid < y) {
                return Ok(false);
            }
            stack.push((mid, y, depth - 1));
            stack.push((x, mid, depth - 1));
        }
        Ok(true)
    }

    /// Whether the two curves are proved to stay within `radius` of each other over their
    /// intervals: every point of each within `radius` of the other or in one of the balls
    /// `ends` ([`Curve::proved_within`] both ways).
    ///
    /// # Errors
    ///
    /// Those of [`Curve::proved_within`].
    pub fn proved_together(
        &self,
        k: &Kernel,
        interval: &Extent1d,
        other: &Self,
        other_interval: &Extent1d,
        radius: f64,
        ends: &[(Vec3, f64)],
    ) -> Result<bool> {
        Ok(
            self.proved_within(k, interval, other, other_interval, radius, ends)?
                && other.proved_within(k, other_interval, self, interval, radius, ends)?,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curve::ellipse::{Ellipse, EllipseOptions};
    use crate::curve::line::Line;
    use crate::math::Point3;

    /// A full circle or ellipse about `centre` in the plane z = `centre.z`.
    fn ellipse(k: &Kernel, centre: Point3, rx: f64, ry: f64) -> Curve {
        let domain = Extent1d::from_min_max(0.0, 360.0);
        let opts = EllipseOptions::default();
        Curve::from(Ellipse::new(k, centre, Vec3::X, Vec3::Y, domain, rx, ry, &opts).unwrap())
    }

    /// The parameter of `curve` nearest `p`.
    fn at(k: &Kernel, curve: &Curve, p: Point3) -> f64 {
        let ivl = curve.natural_interval();
        curve
            .drop_point(k, &ivl, p, None, 1e-9, None, SolverOperation::Minimize)
            .unwrap()
            .parameter
    }

    /// The seat tube's end circle and the top tube's wall cut by its plane (WR-579): a
    /// circle of radius 16 and an ellipse of radii 71 and 16 whose centres are 1.6e-3
    /// apart along the long axis. Near their common top, `y = 16 - u^2 / 32` against
    /// `16 - 16 (u - 1.6e-3)^2 / (2 71^2)`: 4.1e-9 apart at `u = 0`, 1.2e-7 at `u = 2e-3`,
    /// 4.7e-6 at `u = 1.25e-2`.
    #[test]
    fn a_circle_and_an_ellipse_through_one_top_are_together_as_far_as_their_gap() {
        let k = Kernel::default();
        let circle = ellipse(&k, Point3::new(0.0, 0.0, 0.0), 16.0, 16.0);
        let other = ellipse(&k, Point3::new(1.6e-3, 0.0, 0.0), 71.0, 16.0);
        let on_circle = |u: f64| Point3::new(u, (256.0 - u * u).sqrt(), 0.0);
        let on_other = |u: f64| {
            let s = (u - 1.6e-3) / 71.0;
            Point3::new(u, 16.0 * (1.0 - s * s).sqrt(), 0.0)
        };
        let piece = |c: &Curve, p: Point3, q: Point3| {
            let (s, t) = (at(&k, c, p), at(&k, c, q));
            Extent1d::from_min_max(s.min(t), s.max(t))
        };
        let gap = |u: f64| (on_circle(u).y - on_other(u).y).abs();
        // from the corner to the crossing, the stretch of WR-579
        let (u0, u1) = (-8.6e-5, 2.94e-4);
        let (ic, io) = (
            piece(&circle, on_circle(u0), on_circle(u1)),
            piece(&other, on_other(u0), on_other(u1)),
        );
        let widest = gap(0.0).max(gap(u0)).max(gap(u1));
        assert!(widest < 5e-9, "{widest}");
        assert!(circle
            .proved_together(&k, &ic, &other, &io, 1e-8, &[])
            .unwrap());
        assert!(!circle
            .proved_together(&k, &ic, &other, &io, 1e-9, &[])
            .unwrap());
        // a longer stretch parts farther: together within 5e-6, not within 2e-6
        let (u0, u1) = (0.0, 1.25e-2);
        let (ic, io) = (
            piece(&circle, on_circle(u0), on_circle(u1)),
            piece(&other, on_other(u0), on_other(u1)),
        );
        assert!((gap(u1) - 4.7e-6).abs() < 1e-7, "{}", gap(u1));
        assert!(circle
            .proved_together(&k, &ic, &other, &io, 5e-6, &[])
            .unwrap());
        assert!(!circle
            .proved_together(&k, &ic, &other, &io, 2e-6, &[])
            .unwrap());
    }

    /// A chord of a circle strays from its arc by the sagitta, and the arc from the chord
    /// by as much: 1.25e-3 for a chord 0.2 long on a radius of 4.
    #[test]
    fn a_chord_and_its_arc_are_together_within_the_sagitta() {
        let k = Kernel::default();
        let circle = ellipse(&k, Point3::new(1.0, -2.0, 3.0), 4.0, 4.0);
        let half = (0.1_f64 / 4.0).asin();
        // about the circle's top, away from its seam
        let p = Point3::new(1.0 - 0.1, -2.0 + 4.0 * half.cos(), 3.0);
        let q = Point3::new(1.0 + 0.1, -2.0 + 4.0 * half.cos(), 3.0);
        let line = Curve::from(Line::from_points(p, q, 3));
        let (s, t) = (at(&k, &circle, p), at(&k, &circle, q));
        let arc = Extent1d::from_min_max(s.min(t), s.max(t));
        let (s, t) = (at(&k, &line, p), at(&k, &line, q));
        let seg = Extent1d::from_min_max(s.min(t), s.max(t));
        let sagitta = 4.0 * (1.0 - half.cos());
        assert!((sagitta - 1.25e-3).abs() < 1e-6);
        for (a, ia, b, ib) in [(&line, &seg, &circle, &arc), (&circle, &arc, &line, &seg)] {
            assert!(a
                .proved_within(&k, ia, b, ib, sagitta * 1.001, &[])
                .unwrap());
            assert!(!a
                .proved_within(&k, ia, b, ib, sagitta * 0.999, &[])
                .unwrap());
        }
        // the whole circle is not within the chord's reach, nor of a kind refused
        let whole = circle.natural_interval();
        assert!(!circle
            .proved_within(&k, &whole, &line, &seg, 1.0, &[])
            .unwrap());
        assert!(!line
            .proved_within(&k, &seg, &circle, &arc, f64::NAN, &[])
            .unwrap());
        // within half the sagitta only where the chord is in a ball about an end of the arc:
        // balls that reach the chord's middle hold all of it, shorter ones leave the middle
        let half_way = 0.5 * sagitta;
        let balls = |r: f64| [(p, r), (q, r)];
        let within = |ends: &[(Vec3, f64)]| {
            line.proved_within(&k, &seg, &circle, &arc, half_way, ends)
                .unwrap()
        };
        assert!(!within(&[]));
        assert!(within(&balls(0.11)));
        assert!(!within(&balls(0.04)));
    }
}
