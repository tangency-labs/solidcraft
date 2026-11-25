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
//! | [`Curve::proved_near`]     | a line, a B-spline, and a conic by its rational B-spline |
//! | [`Curve::proved_together`] | both ways                                               |
//! | [`Curve::proved_overlap`]  | where two curves run together, the run proved both ways |
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
//! At a full-multiplicity interior knot, the one-sided endpoints must coincide for a
//! piece touching the knot to receive a tube.
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
//! **A seam.** A closed curve's two ends are one point, its seam, and a point about the seam
//! drops on either end of its interval. Where the other curve is closed within the radius
//! (its ends within the radius of each other), the piece of it between two feet may run
//! either way round: between the feet, or from the higher foot to the interval's end and
//! from its start to the lower one. Each of those parts is a piece of the other curve, so
//! each gives a true bound and the least is kept. This curve is cut where the other's seam
//! drops on it, so that no piece of it runs past the other's seam and the pieces on either
//! side are each faced by one of those parts (#18163).
//!
//! **The run.** Two curves that run together over part of each (two sheets' edges along one
//! seam, one shorter or shifted along the other) meet over a run whose ends are ends of the
//! curves: at each end of the run, one curve ends on the other. [`Curve::proved_overlap`] drops
//! each curve's two ends on the other; the ends that land within the radius bound the run
//! (on this curve, the first and last of them by its parameter), and the run is then proved by
//! [`Curve::proved_together`] over its two intervals. The drops only find where to look: a run
//! is reported only as proved, or as found and not proved.
//!
//! **A loop.** Where the first and last of those ends lie within the radius of each other, the
//! run's two ends are one point. Between them this curve either stays at that point (the
//! curves touch) or goes round a loop back to it: the whole of a closed curve, whose two ends
//! are its seam (#18163). Its parameters alone cannot tell which: two closed curves that touch
//! where both their seams are have both of each curve's ends at the point of contact, as two
//! coincident ones do. The middle of the run's interval tells: a loop leaves the ball of the
//! radius about its ends there, and runs along the other curve only if the middle lies within
//! the radius of it. The run then spans the ends' parameters on each curve (on a closed curve
//! the seam's foot may fall at either end of its interval, so the first and last ends by this
//! curve's parameter need not hold the other's), and the other runs the loop's way where their
//! tangents at the middle and its foot agree.
//!
//! Deviations: not a port. Curves of other kinds (offset, composite, iso, projected and
//! curve-on-surface curves, a Hermite curve, a conic without its B-spline) have no hull
//! here, and nothing is proved for them. No recursion: the pieces wait on a stack bounded
//! by [`MAX_PIECES`].

use crate::curve::curve::{Curve, CurveKind};
use crate::curve::length_bound::Span;
use crate::error::Result;
use crate::geom::certified::{next_down, next_up};
use crate::geom::extent1d::Extent1d;
use crate::geom::extent3d::Extent3d;
use crate::geom::global_solver::SolverOperation;
use crate::geom::vector3d::Vec3;
use crate::kernel::Kernel;
use crate::math::EFF_ZERO;
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::math::gamma;

const OP: &str = "curve::proximity::proved_near";
const OP_DISJOINT: &str = "curve::proximity::proved_disjoint";

/// How many times a piece is halved at most.
pub const DEPTH: usize = 32;

/// How many windows, each four times the last, [`Curve::crossing_reach`] tries.
pub const CROSSING_WINDOWS: usize = 8;

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

/// A whole native piece's convex hull, including each coordinate's outward
/// rounding allowance. No sampled or approximation box participates in separation.
fn native_piece_box(curve: &Curve, domain: &Extent1d) -> Option<Extent3d> {
    let points = curve.piece_hull_points(domain)?;
    let mut bounds = Extent3d::new();
    for (p, error) in points {
        if !(p.x.is_finite()
            && p.y.is_finite()
            && p.z.is_finite()
            && error.is_finite()
            && error >= 0.0)
        {
            return None;
        }
        bounds.add_point(Vec3::new(
            next_down(p.x - error),
            next_down(p.y - error),
            next_down(p.z - error),
        ));
        bounds.add_point(Vec3::new(
            next_up(p.x + error),
            next_up(p.y + error),
            next_up(p.z + error),
        ));
    }
    bounds.is_defined().then_some(bounds)
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

/// The foot of `p` on `curve` over `interval`: its parameter, clamped to the interval, and the
/// drop's distance. Where to look, never part of a proof (the drop snaps a foot to an end of
/// the interval only within the scaled zero). `None` where the drop finds none.
fn foot_on(k: &Kernel, curve: &Curve, interval: &Extent1d, p: Vec3) -> Result<Option<(f64, f64)>> {
    let zero = EFF_ZERO * (1.0 + p.x.abs().max(p.y.abs()).max(p.z.abs()));
    let dropped =
        soft(curve.drop_point(k, interval, p, None, zero, None, SolverOperation::Minimize))?;
    Ok(dropped
        .filter(|d| d.success && d.parameter.is_finite())
        .map(|d| {
            (
                d.parameter.max(interval.min()).min(interval.max()),
                d.distance,
            )
        }))
}

/// Where two curves run together ([`Curve::proved_overlap`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Overlap {
    /// No run: fewer than two ends of the curves lie within the radius of the other, or the
    /// ends that do are within the radius of each other and the curves touch at a point there
    /// or meet only there (module docs, "A loop").
    Apart,
    /// The curves run together over the run, proved: every point of each part within the
    /// radius of the other part.
    Proved(Run),
    /// The run's ends were found within the radius, but the run is not proved: the curves may
    /// part between its ends, be of a kind without a hull, or be closer than the pieces tried
    /// could show.
    Unproved(Run),
}

/// The run of two curves ([`Overlap`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Run {
    /// The run's part of this curve's interval, then of the other's.
    pub intervals: [Extent1d; 2],
    /// The same as fractions of the intervals given (0 at the start, 1 at the end), this
    /// curve's, then the other's, each `[low, high]`.
    pub fractions: [[f64; 2]; 2],
    /// Whether the other curve runs along the run in this curve's sense.
    pub same_sense: bool,
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

/// The chord-to-curve part of the tube proof requires a connected spline piece.
/// At a knot of multiplicity degree + 1, the two one-sided limits are separate control
/// points. Equal homogeneous points prove the limits agree; otherwise decline the tube.
/// Check the closed interval so that a trim endpoint cannot silently use the other limit.
fn spline_piece_connected(c: &BSplineCurve, a: f64, b: f64) -> Result<bool> {
    let nurb = c.nurb_ref()?;
    let knots = nurb.knots();
    let Some((&first, &last)) = knots.first().zip(knots.last()) else {
        return Ok(false);
    };
    // Use the actual knots. Grouping nearby values by a tolerance can hide a break.
    let mut start = knots.partition_point(|&t| t < a || t == first);
    let end = knots.partition_point(|&t| t <= b && t < last);
    let Some(inside) = knots.get(start..end) else {
        return Ok(false);
    };
    for run in inside.chunk_by(|x, y| x == y) {
        if run.len() > nurb.degree() {
            let endpoints = start
                .checked_sub(1)
                .and_then(|i| nurb.cpts().get(i))
                .zip(nurb.cpts().get(start));
            if run.len() - 1 != nurb.degree() || !endpoints.is_some_and(|(p, q)| p == q) {
                return Ok(false);
            }
        }
        start += run.len();
    }
    Ok(true)
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
                if !spline_piece_connected(c, a, b)? {
                    return Ok(None);
                }
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

/// A cone that holds every tangent direction of a curve piece, and the piece's two ends.
#[derive(Clone, Copy, Debug)]
pub(crate) struct TangentCone {
    /// The cone's unit axis (the direction of the piece's chord).
    pub(crate) axis: Vec3,
    /// Its half angle in radians, rounding included.
    pub(crate) half_angle: f64,
    /// The piece's ends, each within `rounding` of the exact one.
    pub(crate) ends: [Vec3; 2],
    /// The rounding bound of the ends (a length).
    pub(crate) rounding: f64,
}

/// The angle between `a` and `b`, by `atan2` of the cross and the dot products.
fn angle(a: Vec3, b: Vec3) -> f64 {
    a.cross(b).length().atan2(a.dot(b))
}

impl Curve {
    /// Proves that two native curves have no common point. Each parameter rectangle
    /// is discarded only when outward native hull boxes separate it on a coordinate axis.
    /// Subdivision changes the enclosure, never a geometric tolerance. Unsupported
    /// definitions or exhausted work leave separation unproved.
    ///
    /// # Errors
    /// The kernel's cancellation.
    pub(crate) fn proved_disjoint(
        &self,
        k: &Kernel,
        interval: Extent1d,
        other: &Self,
        other_interval: Extent1d,
    ) -> Result<bool> {
        for (curve, domain) in [(self, interval), (other, other_interval)] {
            let natural = curve.natural_interval();
            if !(domain.min().is_finite()
                && domain.max().is_finite()
                && natural.min() <= domain.min()
                && domain.min() < domain.max()
                && domain.max() <= natural.max())
            {
                return Ok(false);
            }
        }
        let mut work = vec![(interval, other_interval, 0_usize)];
        let mut visits = 0;
        while let Some((a, b, depth)) = work.pop() {
            k.check_cancelled(OP_DISJOINT)?;
            visits += 1;
            if visits > MAX_PIECES {
                return Ok(false);
            }
            let (Some(x), Some(y)) = (native_piece_box(self, &a), native_piece_box(other, &b))
            else {
                return Ok(false);
            };
            let (lo, hi, other_lo, other_hi) = (x.min(), x.max(), y.min(), y.max());
            if hi.x < other_lo.x
                || other_hi.x < lo.x
                || hi.y < other_lo.y
                || other_hi.y < lo.y
                || hi.z < other_lo.z
                || other_hi.z < lo.z
            {
                continue;
            }
            // Twice the one-parameter depth allows alternating splits of both curves.
            if depth >= 2 * DEPTH {
                return Ok(false);
            }
            let split_first = x.size().length_squared() >= y.size().length_squared();
            let domain = if split_first { a } else { b };
            let mid = domain.mid();
            if !(domain.min() < mid && mid < domain.max()) {
                return Ok(false);
            }
            for piece in [
                Extent1d::from_min_max(domain.min(), mid),
                Extent1d::from_min_max(mid, domain.max()),
            ] {
                work.push(if split_first {
                    (piece, b, depth + 1)
                } else {
                    (a, piece, depth + 1)
                });
            }
        }
        Ok(true)
    }

    /// A cone holding every tangent direction of this curve over `[a, b]` (`a < b`), from
    /// its control points: a line's direction; for a B-spline (and a conic by its rational
    /// B-spline) the tangent of a Bezier piece with positive weights is a combination with
    /// non-negative coefficients of the differences `P_j - P_i`, `i < j`, of its control
    /// points (Floater, "Derivatives of rational Bezier curves", CAGD 9, 1992), so the cone
    /// about the chord that holds those differences, each widened by the `asin` of its
    /// points' rounding over its length, holds the tangent. `None` for a kind without a
    /// hull, a piece of no length, a difference no longer than its rounding (its direction
    /// is not known), or a cone of a quarter turn or more.
    ///
    /// # Errors
    ///
    /// The kernel's cancellation.
    pub(crate) fn tangent_cone(&self, a: f64, b: f64) -> Result<Option<TangentCone>> {
        if !(a.is_finite() && b.is_finite() && a < b) {
            return Ok(None);
        }
        let Some(form) = form(self) else {
            return Ok(None);
        };
        let sqrt3 = 3.0_f64.sqrt();
        // the pieces' control points, each piece with the rounding of its points
        let mut pieces: Vec<(Vec<Vec3>, f64)> = Vec::new();
        match &form {
            Form::Line(l) => {
                let Some([(p, ep), (q, eq)]) = l.segment_over(&Extent1d::from_min_max(a, b)) else {
                    return Ok(None);
                };
                pieces.push((vec![p, q], ep.max(eq)));
            }
            Form::Spline(c) => {
                let Some(cuts) = soft(spline_breaks(c, a, b))? else {
                    return Ok(None);
                };
                for w in cuts.windows(2) {
                    let [x, y] = *w else { continue };
                    if y <= x {
                        continue;
                    }
                    let Some(hull) =
                        soft(Span::of(c, 0.5 * (x + y)))?.and_then(|span| span.hull(x, y))
                    else {
                        return Ok(None);
                    };
                    pieces.push(hull);
                }
            }
        }
        let (Some(first), Some(last)) = (
            pieces.first().and_then(|(p, _)| p.first().copied()),
            pieces.last().and_then(|(p, _)| p.last().copied()),
        ) else {
            return Ok(None);
        };
        let rounding = pieces.iter().fold(0.0_f64, |m, (_, e)| m.max(*e)) * sqrt3;
        let chord = last - first;
        let length = chord.length();
        if length.is_nan() || length <= 2.0 * rounding {
            return Ok(None);
        }
        let axis = chord / length;
        // the chord's own direction to its rounding
        let mut half_angle = (2.0 * rounding / length).min(1.0).asin();
        let own = half_angle;
        for (points, _) in &pieces {
            for (i, &p) in points.iter().enumerate() {
                for &q in points.iter().skip(i + 1) {
                    let d = q - p;
                    let len = d.length();
                    if len.is_nan() || len <= 2.0 * rounding {
                        return Ok(None);
                    }
                    let wide = angle(axis, d) + (2.0 * rounding / len).min(1.0).asin() + own;
                    half_angle = half_angle.max(wide * (1.0 + gamma(8)));
                }
            }
        }
        if half_angle.is_nan() || half_angle >= 0.5 * std::f64::consts::PI {
            return Ok(None);
        }
        Ok(Some(TangentCone {
            axis,
            half_angle,
            ends: [first, last],
            rounding,
        }))
    }

    /// The parameters to cut `[a, b]` at so that each piece has one hull (a line whole, a
    /// B-spline or a conic at its knots); `None` for a kind without a hull or a curve that
    /// cannot be read.
    ///
    /// # Errors
    ///
    /// The kernel's cancellation.
    pub(crate) fn hull_breaks(&self, a: f64, b: f64) -> Result<Option<Vec<f64>>> {
        if !(a.is_finite() && b.is_finite() && a <= b) {
            return Ok(None);
        }
        let Some(form) = form(self) else {
            return Ok(None);
        };
        soft(form.breaks(a, b))
    }

    /// A ball that holds the piece of this curve over `[a, b]`: the middle of the piece's
    /// chord, and half the chord plus the piece's hull radius about it (module docs, "A
    /// tube"), the middle's and the length's rounding included. `None` where the piece has
    /// no certified hull.
    ///
    /// # Errors
    ///
    /// The kernel's cancellation.
    pub(crate) fn piece_ball(&self, a: f64, b: f64) -> Result<Option<(Vec3, f64)>> {
        if !(a.is_finite() && b.is_finite() && a <= b) {
            return Ok(None);
        }
        let Some(form) = form(self) else {
            return Ok(None);
        };
        let Some(tube) = soft(form.tube(a, b))?.flatten() else {
            return Ok(None);
        };
        let centre = (tube.a + tube.b) * 0.5;
        let half = 0.5 * (tube.b - tube.a).length();
        let size = tube.a.length() + tube.b.length();
        let radius = (half + tube.rho + gamma(4) * size) * (1.0 + gamma(4));
        Ok(radius.is_finite().then_some((centre, radius)))
    }

    /// How far along `edge` the crossing of this curve with it is known, enclosed: this
    /// curve at parameter `t` and `edge` at `s` meet at `p`, each known to `tolerance`
    /// across itself. Over a stretch about `p` on which every tangent of this curve makes
    /// at least the angle `theta` with every tangent of `edge`, a curve moved by
    /// `tolerance` moves the crossing by at most `tolerance / sin theta` along the edge.
    /// The stretch is a parameter window about `t` and `s`, widened until the reach
    /// `tolerance / sin theta` lies inside what both windows cover (a curve whose tangents
    /// stay in a cone of half angle `alpha` about an axis leaves its window no nearer `p`
    /// than its window's end times `cos alpha`; an end of the curve's own domain covers
    /// everything). `theta` is the angle between the two cones' axes (as lines) less both
    /// half angles ([`Curve::tangent_cone`]). Returns the reach; `None` where no window
    /// proves one: the curves may run along each other, or have no hull. How far `edge`
    /// strays from its tangent line within the reach is [`Curve::stray_from_line`].
    ///
    /// # Errors
    ///
    /// The kernel's cancellation.
    pub(crate) fn crossing_reach(
        &self,
        k: &Kernel,
        t: f64,
        edge: &Self,
        s: f64,
        p: Vec3,
        tolerance: f64,
    ) -> Result<Option<f64>> {
        if !(tolerance > 0.0 && tolerance.is_finite() && t.is_finite() && s.is_finite()) {
            return Ok(None);
        }
        let domains = [self.natural_interval(), edge.natural_interval()];
        // the window's first half width on each curve: a few tolerances of length
        let speed = |c: &Self, at: f64| -> Result<Option<f64>> {
            Ok(soft(c.evaluate(k, at, 1, true, false))?
                .and_then(|d| d.get(1).map(|v| v.length()))
                .filter(|v| v.is_finite() && *v > 0.0))
        };
        let (Some(v_c), Some(v_e)) = (speed(self, t)?, speed(edge, s)?) else {
            return Ok(None);
        };
        let mut width = 4.0 * tolerance;
        for _ in 0..CROSSING_WINDOWS {
            k.check_cancelled(OP)?;
            let mut cones = Vec::new();
            for (c, at, v, domain) in [(self, t, v_c, domains[0]), (edge, s, v_e, domains[1])] {
                let h = width / v;
                let (lo, hi) = ((at - h).max(domain.min()), (at + h).min(domain.max()));
                let Some(cone) = c.tangent_cone(lo, hi)? else {
                    return Ok(None);
                };
                // how far from `p` the window is sure to hold the curve
                let cos = cone.half_angle.cos();
                let mut covered = f64::INFINITY;
                for (end, limit, bound) in [
                    (cone.ends[0], lo, domain.min()),
                    (cone.ends[1], hi, domain.max()),
                ] {
                    if (limit - bound).abs() > 0.0 {
                        let reach = ((end - p).length() - cone.rounding) * cos * (1.0 - gamma(8));
                        covered = covered.min(reach);
                    }
                }
                cones.push((cone, covered));
            }
            let [(cone_c, covered_c), (cone_e, covered_e)] = cones.as_slice() else {
                return Ok(None);
            };
            // the angle between the two axes as lines, less both cones
            let between = angle(cone_c.axis, cone_e.axis);
            let acute = between.min(std::f64::consts::PI - between);
            let theta = acute * (1.0 - gamma(8)) - cone_c.half_angle - cone_e.half_angle;
            if theta > 0.0 {
                let reach = tolerance / theta.sin() * (1.0 + gamma(8));
                if reach <= covered_c.min(*covered_e) {
                    return Ok(Some(reach));
                }
            }
            width *= 4.0;
        }
        Ok(None)
    }

    /// How far this curve strays from the line through `p` along `along` (a tangent of the
    /// curve at parameter `s`, `p` a point near the curve there), enclosed over the curve
    /// from `s` out to `reach` from `p` either way. On each side of `s` a parameter window
    /// is widened until it is sure to hold the curve out to `reach` from `p` (its far end
    /// times the cosine of its cone, as [`Curve::crossing_reach`]) or ends at the end of
    /// the domain. Every tangent over the window lies in its cone of half angle `alpha`
    /// about the axis ([`Curve::tangent_cone`]), so every point `q` of the window lies in
    /// that cone from the curve's point `c` at `s` (the window's end there, within the
    /// cone's rounding), and `q - c` makes at most `phi`, the angle between the axis and
    /// `along` as lines plus `alpha`, with the line. A point within `reach` of `p` is
    /// within `reach + |c - p|` of `c`, so within `|c - p| + (reach + |c - p|) sin phi` of
    /// the line. `None` where a window has no cone, none holds the reach, or `phi` reaches
    /// a quarter turn: nothing is enclosed.
    ///
    /// # Errors
    ///
    /// The kernel's cancellation.
    pub(crate) fn stray_from_line(
        &self,
        k: &Kernel,
        s: f64,
        along: Vec3,
        p: Vec3,
        reach: f64,
    ) -> Result<Option<f64>> {
        let finite = [s, reach, along.x, along.y, along.z, p.x, p.y, p.z]
            .iter()
            .all(|v| v.is_finite());
        if !(finite && reach > 0.0 && along.length() > 0.0) {
            return Ok(None);
        }
        let domain = self.natural_interval();
        let Some(speed) = soft(self.evaluate(k, s, 1, true, false))?
            .and_then(|d| d.get(1).map(|v| v.length()))
            .filter(|v| v.is_finite() && *v > 0.0)
        else {
            return Ok(None);
        };
        let mut stray = 0.0_f64;
        for upper in [false, true] {
            let mut width = 1.25 * reach;
            let mut held = None;
            for _ in 0..CROSSING_WINDOWS {
                k.check_cancelled(OP)?;
                let h = width / speed;
                let (lo, hi) = if upper {
                    (s, (s + h).min(domain.max()))
                } else {
                    ((s - h).max(domain.min()), s)
                };
                if lo >= hi {
                    if (upper && s >= domain.max()) || (!upper && s <= domain.min()) {
                        // `s` is the domain's end on this side: no curve beyond it
                        held = Some(None);
                        break;
                    }
                    width *= 4.0;
                    continue;
                }
                let Some(cone) = self.tangent_cone(lo, hi)? else {
                    return Ok(None);
                };
                let (near, far, far_t, bound) = if upper {
                    (cone.ends[0], cone.ends[1], hi, domain.max())
                } else {
                    (cone.ends[1], cone.ends[0], lo, domain.min())
                };
                let covered = if (far_t - bound).abs() > 0.0 {
                    ((far - p).length() - cone.rounding) * cone.half_angle.cos() * (1.0 - gamma(8))
                } else {
                    f64::INFINITY
                };
                if covered >= reach {
                    held = Some(Some((cone, near)));
                    break;
                }
                width *= 4.0;
            }
            let Some(window) = held else {
                return Ok(None);
            };
            let Some((cone, near)) = window else {
                continue;
            };
            let between = angle(cone.axis, along);
            let acute = between.min(std::f64::consts::PI - between);
            let phi = (acute + cone.half_angle) * (1.0 + gamma(8)) + gamma(8);
            if phi.is_nan() || phi >= 0.5 * std::f64::consts::PI {
                return Ok(None);
            }
            let off = ((near - p).length() + cone.rounding) * (1.0 + gamma(4));
            let side = (off + (reach + off) * phi.sin()) * (1.0 + gamma(4)) + gamma(4) * reach;
            stray = stray.max(side);
        }
        Ok((!stray.is_nan()).then_some(stray))
    }

    /// Whether every point of this curve over `interval` is proved to lie within `radius`
    /// of `other` over `other_interval`, or within one of the balls `ends` (a centre and a
    /// radius each: the vertices `other` runs between, with their tolerances, which hold
    /// its ends) (module docs). `false` proves nothing: the curves may be farther apart, of
    /// a kind without a hull, or closer than the pieces tried could show.
    ///
    /// # Errors
    ///
    /// The kernel's cancellation (a failed evaluation or point drop proves nothing).
    pub fn proved_near(
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
        let Some(mut cuts) = soft(mine.breaks(a, b))? else {
            return Ok(false);
        };
        // the other curve closed within the radius: this one is cut where its seam drops
        // (module docs, "A seam")
        let closed = soft(other.is_closed(k, other_interval, radius))?.unwrap_or(false);
        if closed {
            if let Some(seam) = soft(other.evaluate_point(k, lo))? {
                if let Some((t, _)) = foot_on(k, self, interval, seam)? {
                    if a < t && t < b && !cuts.contains(&t) {
                        cuts.push(t);
                        cuts.sort_by(f64::total_cmp);
                    }
                }
            }
        }
        let mut stack: Vec<(f64, f64, usize)> = cuts
            .windows(2)
            .filter_map(|w| match *w {
                [x, y] => Some((x, y, DEPTH)),
                _ => None,
            })
            .collect();
        // the foot of a point on the other curve: where to look, not part of the proof
        let foot = |p: Vec3| -> Result<Option<f64>> {
            Ok(foot_on(k, other, other_interval, p)?.map(|(t, _)| t))
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
            // the bound against the other curve's piece over `[u, v]`
            let against = |u: f64, v: f64| -> Result<Option<f64>> {
                Ok(soft(theirs.tube(u, v))?.flatten().map(|facing| {
                    let reach = segment_distance(piece.a, facing.a, facing.b)
                        .max(segment_distance(piece.b, facing.a, facing.b));
                    (piece.rho + reach + facing.rho) * (1.0 + gamma(3))
                }))
            };
            let (low, high) = (s.min(t), s.max(t));
            let Some(mut bound) = against(low, high)? else {
                return Ok(false);
            };
            if closed && bound > radius {
                // the other curve's piece round its seam (module docs, "A seam")
                for (u, v) in [(lo, low), (high, hi)] {
                    if let Some(round) = against(u, v)? {
                        bound = bound.min(round);
                    }
                }
            }
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

    /// Whether the proofs here can enclose this curve at all (a line, or a B-spline form
    /// with its NURB: a B-spline, ellipse, hyperbola or parabola). Of any other kind
    /// ([`Curve::proved_near`] and [`Curve::proved_together`]) nothing is proved.
    pub(crate) fn can_be_proved(&self) -> bool {
        form(self).is_some()
    }

    /// Whether the two curves are proved to stay within `radius` of each other over their
    /// intervals: every point of each within `radius` of the other or in one of the balls
    /// `ends` ([`Curve::proved_near`] both ways).
    ///
    /// # Errors
    ///
    /// Those of [`Curve::proved_near`].
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
            self.proved_near(k, interval, other, other_interval, radius, ends)?
                && other.proved_near(k, other_interval, self, interval, radius, ends)?,
        )
    }

    /// Where this curve over `interval` and `other` over `other_interval` run together within
    /// `radius` (module docs, "The run"): each curve's two ends dropped on the other, those
    /// within `radius` bounding the run, and the run proved by [`Curve::proved_together`].
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`](crate::error::SolidError::InvalidInput) for a radius not finite
    /// and positive or an interval not finite or reversed; the evaluations' errors and those
    /// of the proof (its cancellation).
    pub fn proved_overlap(
        &self,
        k: &Kernel,
        interval: &Extent1d,
        other: &Self,
        other_interval: &Extent1d,
        radius: f64,
    ) -> Result<Overlap> {
        let bounds = [
            interval.min(),
            interval.max(),
            other_interval.min(),
            other_interval.max(),
        ];
        if !(bounds.iter().all(|v| v.is_finite())
            && interval.min() <= interval.max()
            && other_interval.min() <= other_interval.max()
            && radius.is_finite()
            && radius > 0.0)
        {
            return Err(crate::error::SolidError::invalid_input(
                "curve::proximity::proved_overlap",
                "a radius not finite and positive, or an interval not finite or reversed",
            ));
        }
        // each end within the radius of the other curve: (this curve's parameter, the
        // other's, the point)
        let mut ends: Vec<(f64, f64, Vec3)> = Vec::new();
        for t in [interval.min(), interval.max()] {
            let p = self.evaluate_point(k, t)?;
            if let Some((s, d)) = foot_on(k, other, other_interval, p)? {
                if d <= radius {
                    ends.push((t, s, p));
                }
            }
        }
        for s in [other_interval.min(), other_interval.max()] {
            let q = other.evaluate_point(k, s)?;
            if let Some((t, d)) = foot_on(k, self, interval, q)? {
                if d <= radius {
                    ends.push((t, s, q));
                }
            }
        }
        let first = ends.iter().min_by(|a, b| a.0.total_cmp(&b.0));
        let last = ends.iter().max_by(|a, b| a.0.total_cmp(&b.0));
        let (Some(&(t0, s0, p0)), Some(&(t1, s1, p1))) = (first, last) else {
            return Ok(Overlap::Apart);
        };
        let (mine, theirs, same_sense) = if (p1 - p0).length() > radius {
            // the run lies between its two ends
            (
                Extent1d::from_min_max(t0, t1),
                Extent1d::from_min_max(s0.min(s1), s0.max(s1)),
                s0 <= s1,
            )
        } else {
            // its ends are one point: a point of contact, or a loop through it
            let span = |pick: fn(&(f64, f64, Vec3)) -> f64| {
                ends.iter()
                    .map(pick)
                    .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| {
                        (lo.min(v), hi.max(v))
                    })
            };
            let (t_lo, t_hi) = span(|e| e.0);
            let (s_lo, s_hi) = span(|e| e.1);
            let mine = Extent1d::from_min_max(t_lo, t_hi);
            let theirs = Extent1d::from_min_max(s_lo, s_hi);
            match self.loop_along(k, &mine, other, &theirs, p0, radius)? {
                Some(same_sense) => (mine, theirs, same_sense),
                None => return Ok(Overlap::Apart),
            }
        };
        let fraction = |ivl: &Extent1d, v: f64| ivl.inversion(v).parameter;
        let (Some(a0), Some(a1), Some(b0), Some(b1)) = (
            fraction(interval, mine.min()),
            fraction(interval, mine.max()),
            fraction(other_interval, theirs.min()),
            fraction(other_interval, theirs.max()),
        ) else {
            return Ok(Overlap::Apart);
        };
        let run = Run {
            intervals: [mine, theirs],
            fractions: [[a0, a1], [b0, b1]],
            same_sense,
        };
        Ok(
            if self.proved_together(k, &mine, other, &theirs, radius, &[])? {
                Overlap::Proved(run)
            } else {
                Overlap::Unproved(run)
            },
        )
    }

    /// Whether this curve over `mine`, whose ends both lie at `at` (to `radius`), is a loop
    /// along `other` over `theirs` (module docs, "A loop"): its middle leaves the ball of
    /// radius `radius` about `at` and lies within `radius` of `other`. `Some` with whether
    /// `other` runs the loop's way there (their tangents at the middle and its foot), `None`
    /// for a point of contact, a loop away from `other` or a failed evaluation. The drop only
    /// finds where to look.
    fn loop_along(
        &self,
        k: &Kernel,
        mine: &Extent1d,
        other: &Self,
        theirs: &Extent1d,
        at: Vec3,
        radius: f64,
    ) -> Result<Option<bool>> {
        let middle = 0.5 * (mine.min() + mine.max());
        let Some(at_middle) = soft(self.evaluate(k, middle, 1, false, false))? else {
            return Ok(None);
        };
        let &[p, dp, ..] = at_middle.as_slice() else {
            return Ok(None);
        };
        if (p - at).length() <= radius {
            return Ok(None);
        }
        let Some((s, d)) = foot_on(k, other, theirs, p)? else {
            return Ok(None);
        };
        if d > radius {
            return Ok(None);
        }
        let Some(at_foot) = soft(other.evaluate(k, s, 1, false, false))? else {
            return Ok(None);
        };
        let &[_, dq, ..] = at_foot.as_slice() else {
            return Ok(None);
        };
        Ok(Some(dp.dot(dq) > 0.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curve::ellipse::{Ellipse, EllipseOptions};
    use crate::curve::line::Line;
    use crate::math::Point3;

    #[test]
    fn subnormal_split_rounding_cannot_prove_crossing_curves_disjoint() {
        use crate::nurbs::crv_basic::Curve as NurbCurve;
        use crate::nurbs::math::CPoint;

        let k = Kernel::default();
        let domain = Extent1d::from_min_max(0.0, 1.0);
        let left = Extent1d::from_min_max(0.0, 0.5);
        let crossing = Curve::from(Line::from_points(
            Point3::new(3.0 / 10.0, 1.0 / 40.0, 0.0),
            Point3::new(9.0 / 20.0, 1.0 / 40.0, 0.0),
            3,
        ));
        for weight in [f64::from_bits(16), f64::MIN_POSITIVE, 1.0] {
            // Equal weights define exactly (u, u/16, 0). At the smallest
            // weight, splitting at 1/2 rounds the weighted y = 2^-1075 to zero.
            let curve = Curve::from(BSplineCurve::from_nurb(
                NurbCurve::new(
                    vec![
                        CPoint::new(0.0, 0.0, 0.0, weight),
                        CPoint::new(weight, weight / 16.0, 0.0, weight),
                    ],
                    1,
                    vec![0.0, 0.0, 1.0, 1.0],
                )
                .unwrap(),
                3,
                false,
            ));
            if let Some(bounds) = native_piece_box(&curve, &left) {
                assert!(
                    bounds.contains_point(Point3::new(0.5, 1.0 / 32.0, 0.0), 0.0),
                    "weight {weight}: {bounds:?} misses the exact endpoint"
                );
            }
            // At the stored horizontal y = fl(1/40), x = 16y is strictly inside
            // both pieces. This is an exact intersection of the stored inputs.
            assert!(!curve.proved_disjoint(&k, left, &crossing, domain).unwrap());
        }
    }

    #[test]
    fn disjoint_curves_need_whole_span_separation() {
        use crate::nurbs::b_spline_curve::BSplineCurve;
        use crate::nurbs::crv_basic::Curve as NurbCurve;
        use crate::nurbs::math::{CPoint, NOW};

        let k = Kernel::default();
        let seam = Curve::from(Line::from_points(
            Point3::new(-1.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            3,
        ));
        let cubic = |heights: [f64; 4]| {
            let points = [-1.0, -1.0 / 3.0, 1.0 / 3.0, 1.0]
                .into_iter()
                .zip(heights)
                .map(|(x, y)| CPoint::new(x, y, 0.0, NOW))
                .collect();
            Curve::from(BSplineCurve::from_nurb(
                NurbCurve::new(points, 3, vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0]).unwrap(),
                3,
                false,
            ))
        };
        let domain = Extent1d::from_min_max(0.0, 1.0);
        // Negative controls, but y(t) = 1 - 3.3t(1-t) >= 0.175 everywhere.
        let apart = cubic([1.0, -0.1, -0.1, 1.0]);
        assert!(seam.proved_disjoint(&k, domain, &apart, domain).unwrap());
        // All three stations are positive, but y(1/4) = -13/32: two crossings.
        let crossing = cubic([1.0, -3.0, 3.0, 1.0]);
        for t in [0.0, 0.5, 1.0] {
            assert!(crossing.evaluate_point(&k, t).unwrap().y > 0.0);
        }
        assert!(!seam.proved_disjoint(&k, domain, &crossing, domain).unwrap());
        assert!(seam
            .proved_disjoint(&k, domain, &crossing, Extent1d::from_min_max(0.5, 1.0))
            .unwrap());
        assert!(!seam.proved_disjoint(&k, domain, &seam, domain).unwrap());
        let tangent = cubic([3.0, -1.0, -1.0, 3.0]);
        assert!(!seam.proved_disjoint(&k, domain, &tangent, domain).unwrap());
        assert!(!seam
            .proved_disjoint(&k, domain, &apart, Extent1d::from_min_max(-1.0, 1.0))
            .unwrap());
    }

    /// A full circle or ellipse about `centre` in the plane z = `centre.z`.
    fn ellipse(k: &Kernel, centre: Point3, rx: f64, ry: f64) -> Curve {
        circle_along(k, centre, Vec3::X, Vec3::Y, rx, ry)
    }

    /// A full circle or ellipse about `centre`, radius `rx` along `x` (its seam, angle 0, at
    /// `centre + rx x`) and `ry` along `y` (angle 90 degrees).
    fn circle_along(k: &Kernel, centre: Point3, x: Vec3, y: Vec3, rx: f64, ry: f64) -> Curve {
        let domain = Extent1d::from_min_max(0.0, 360.0);
        let opts = EllipseOptions::default();
        Curve::from(Ellipse::new(k, centre, x, y, domain, rx, ry, &opts).unwrap())
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

    #[test]
    fn a_spline_with_a_gap_does_not_prove_an_unbroken_seam() {
        use crate::nurbs::crv_basic::Curve as Nurb;
        use crate::nurbs::math::{CPoint, NOW};

        let k = Kernel::default();
        let line = Curve::from(Line::from_points(Point3::ZERO, Vec3::X, 3));
        let spline = |xs: &[f64], knots: &[f64]| {
            let points = xs.iter().map(|&x| CPoint::new(x, 0.0, 0.0, NOW)).collect();
            let nurb = Nurb::new(points, 1, knots.to_vec()).unwrap();
            Curve::from(BSplineCurve::from_nurb(nurb, 3, true))
        };
        let broken = spline(&[0.0, 0.1, 0.9, 1.0], &[0.0, 0.0, 0.5, 0.5, 1.0, 1.0]);
        let whole = Extent1d::from_min_max(0.0, 1.0);
        // The line midpoint is 0.4 from either component.
        assert!(matches!(
            line.proved_overlap(&k, &whole, &broken, &whole, 0.01)
                .unwrap(),
            Overlap::Unproved(_)
        ));
        assert!(!line
            .proved_near(&k, &whole, &broken, &whole, 0.01, &[])
            .unwrap());
        assert!(!broken
            .proved_together(&k, &whole, &line, &whole, 0.01, &[])
            .unwrap());

        // A subinterval away from the break and a continuous spline still certify.
        let first = Extent1d::from_min_max(0.0, 0.25);
        let first_line = Extent1d::from_min_max(0.0, 0.05);
        assert!(line
            .proved_together(&k, &first_line, &broken, &first, 1e-8, &[])
            .unwrap());
        let joined = spline(&[0.0, 0.5, 1.0], &[0.0, 0.0, 0.5, 1.0, 1.0]);
        assert!(line
            .proved_together(&k, &whole, &joined, &whole, 1e-8, &[])
            .unwrap());
        // A repeated knot is also continuous when its two endpoint controls agree.
        let touching = spline(&[0.0, 0.5, 0.5, 1.0], &[0.0, 0.0, 0.5, 0.5, 1.0, 1.0]);
        assert!(line
            .proved_together(&k, &whole, &touching, &whole, 1e-8, &[])
            .unwrap());
        let broken_form = form(&broken).unwrap();
        assert!(broken_form.tube(0.0, 0.5).unwrap().is_none());
        assert!(broken_form.tube(0.5, 1.0).unwrap().is_none());
        // The last two interior knots coincide, one representable step after the first.
        let t = 0.5_f64.next_up();
        let clustered = spline(&[0.0, 0.1, 0.1, 0.9, 1.0], &[0.0, 0.0, 0.5, t, t, 1.0, 1.0]);
        assert!(form(&clustered).unwrap().tube(0.0, 1.0).unwrap().is_none());
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
            assert!(a.proved_near(&k, ia, b, ib, sagitta * 1.001, &[]).unwrap());
            assert!(!a.proved_near(&k, ia, b, ib, sagitta * 0.999, &[]).unwrap());
        }
        // the whole circle is not within the chord's reach, nor of a kind refused
        let whole = circle.natural_interval();
        assert!(!circle
            .proved_near(&k, &whole, &line, &seg, 1.0, &[])
            .unwrap());
        assert!(!line
            .proved_near(&k, &seg, &circle, &arc, f64::NAN, &[])
            .unwrap());
        // within half the sagitta only where the chord is in a ball about an end of the arc:
        // balls that reach the chord's middle hold all of it, shorter ones leave the middle
        let half_way = 0.5 * sagitta;
        let balls = |r: f64| [(p, r), (q, r)];
        let within = |ends: &[(Vec3, f64)]| {
            line.proved_near(&k, &seg, &circle, &arc, half_way, ends)
                .unwrap()
        };
        assert!(!within(&[]));
        assert!(within(&balls(0.11)));
        assert!(!within(&balls(0.04)));
    }

    /// Two lines along x: one from 0 to 10, one from 5 to 15 (either way along), run together
    /// from 5 to 10, the second half of the first and the first half of the second; a copy
    /// of the first runs with it all along; one half a unit off runs with neither at a
    /// hundredth; one meeting it end to end only touches.
    #[test]
    fn lines_along_one_another_run_together_over_their_shared_part() {
        let k = Kernel::default();
        let line = |a: f64, b: f64, y: f64| {
            Curve::from(Line::from_points(
                Point3::new(a, y, 0.0),
                Point3::new(b, y, 0.0),
                3,
            ))
        };
        let a = line(0.0, 10.0, 0.0);
        let ia = a.natural_interval();
        for (b, same) in [(line(5.0, 15.0, 0.0), true), (line(15.0, 5.0, 0.0), false)] {
            let ib = b.natural_interval();
            let Overlap::Proved(run) = a.proved_overlap(&k, &ia, &b, &ib, 1e-2).unwrap() else {
                panic!("not proved");
            };
            assert_eq!(run.same_sense, same);
            let near = |x: [f64; 2], y: [f64; 2]| {
                (x[0] - y[0]).abs() <= 1e-12 && (x[1] - y[1]).abs() <= 1e-12
            };
            assert!(near(run.fractions[0], [0.5, 1.0]), "{run:?}");
            let theirs = if same { [0.0, 0.5] } else { [0.5, 1.0] };
            assert!(near(run.fractions[1], theirs), "{run:?}");
        }
        let copy = line(0.0, 10.0, 0.0);
        let Overlap::Proved(run) = a
            .proved_overlap(&k, &ia, &copy, &copy.natural_interval(), 1e-2)
            .unwrap()
        else {
            panic!("not proved");
        };
        assert_eq!(run.fractions, [[0.0, 1.0]; 2]);
        let off = line(0.0, 10.0, 0.5);
        assert_eq!(
            a.proved_overlap(&k, &ia, &off, &off.natural_interval(), 1e-2)
                .unwrap(),
            Overlap::Apart
        );
        let after = line(10.0, 20.0, 0.0);
        assert_eq!(
            a.proved_overlap(&k, &ia, &after, &after.natural_interval(), 1e-2)
                .unwrap(),
            Overlap::Apart
        );
        assert!(a.proved_overlap(&k, &ia, &copy, &ia, f64::NAN).is_err());
        assert!(a.proved_overlap(&k, &ia, &copy, &ia, 0.0).is_err());
    }

    /// Two circles of radius 4 about the origin in z = 0 (#18163): a closed curve has no ends,
    /// and its two ends, at its seam, are one point; the run is the whole of both, whether
    /// their seams fall together (angle 0 of both along x), apart (the other's along y) or
    /// the other runs the other way (its angle 90 along -y). A circle that touches it at one
    /// point, (0, 4, 0) away from both seams or (4, 0, 0) where both seams are, is apart.
    #[test]
    fn two_closed_curves_along_one_another_run_together_over_the_whole_of_each() {
        let k = Kernel::default();
        let o = Point3::new(0.0, 0.0, 0.0);
        let a = ellipse(&k, o, 4.0, 4.0);
        let ia = a.natural_interval();
        // 1e-3: the circles' tubes are proved in pieces of about sqrt(8 * 4 * 1e-3) = 0.18
        let radius = 1e-3;
        for (x, y, same) in [
            (Vec3::X, Vec3::Y, true),
            (Vec3::Y, -Vec3::X, true),
            (Vec3::X, -Vec3::Y, false),
        ] {
            let b = circle_along(&k, o, x, y, 4.0, 4.0);
            let ib = b.natural_interval();
            let Overlap::Proved(run) = a.proved_overlap(&k, &ia, &b, &ib, radius).unwrap() else {
                panic!("not proved: x {x:?}, y {y:?}");
            };
            assert_eq!(run.intervals, [ia, ib], "x {x:?}, y {y:?}");
            assert_eq!(run.fractions, [[0.0, 1.0]; 2], "x {x:?}, y {y:?}");
            assert_eq!(run.same_sense, same, "x {x:?}, y {y:?}");
        }
        for (centre, x) in [
            (Point3::new(0.0, 8.0, 0.0), Vec3::X),
            (Point3::new(8.0, 0.0, 0.0), -Vec3::X),
        ] {
            let b = circle_along(&k, centre, x, Vec3::Y, 4.0, 4.0);
            assert_eq!(
                a.proved_overlap(&k, &ia, &b, &b.natural_interval(), radius)
                    .unwrap(),
                Overlap::Apart,
                "{centre:?}"
            );
        }
    }

    /// An arc and its chord share their ends but part by the sagitta between them: at a
    /// radius below it the run's ends are found and the run is not proved; above it, proved.
    #[test]
    fn a_chord_and_its_arc_run_together_only_within_the_sagitta() {
        let k = Kernel::default();
        let circle = ellipse(&k, Point3::new(0.0, 0.0, 0.0), 4.0, 4.0);
        let half = (0.1_f64 / 4.0).asin();
        let p = Point3::new(-0.1, 4.0 * half.cos(), 0.0);
        let q = Point3::new(0.1, 4.0 * half.cos(), 0.0);
        let chord = Curve::from(Line::from_points(p, q, 3));
        let (s, t) = (at(&k, &circle, p), at(&k, &circle, q));
        let arc = Extent1d::from_min_max(s.min(t), s.max(t));
        let sagitta = 4.0 * (1.0 - half.cos());
        let seg = chord.natural_interval();
        assert!(matches!(
            chord
                .proved_overlap(&k, &seg, &circle, &arc, 0.5 * sagitta)
                .unwrap(),
            Overlap::Unproved(_)
        ));
        assert!(matches!(
            chord
                .proved_overlap(&k, &seg, &circle, &arc, 1.01 * sagitta)
                .unwrap(),
            Overlap::Proved(_)
        ));
    }

    /// A line across a circle of radius 2 at the origin: the crossing's reach along the
    /// circle is the tolerance over the sine of the angle, enclosed. At 30 degrees it is
    /// within a hair of `1e-5 / sin 30`; at 0.16 degrees (sine 0.0028), where the first
    /// order gives 3.6e-3, the circle turns by more than that angle within the reach and
    /// nothing is proved; a line along the tangent proves nothing either.
    #[test]
    fn a_crossing_s_reach_is_enclosed_and_a_grazing_one_has_none() {
        let k = Kernel::default();
        let circle = ellipse(&k, Point3::new(0.0, 0.0, 0.0), 2.0, 2.0);
        // the circle's top, away from its seam: tangent along x
        let p = Point3::new(0.0, 2.0, 0.0);
        let s = at(&k, &circle, p);
        let reach = |degrees: f64| {
            let (sin, cos) = degrees.to_radians().sin_cos();
            let d = Vec3::new(cos, sin, 0.0);
            let line = Curve::from(Line::from_points(p - d, p + d, 3));
            let t = at(&k, &line, p);
            line.crossing_reach(&k, t, &circle, s, p, 1e-5).unwrap()
        };
        let steep = reach(30.0).unwrap();
        assert!((2e-5..2.001e-5).contains(&steep), "{steep}");
        let stray = circle
            .stray_from_line(&k, s, Vec3::new(1.0, 0.0, 0.0), p, steep)
            .unwrap()
            .unwrap();
        assert!(stray < 1e-9, "{stray}");
        assert_eq!(reach(0.16), None);
        assert_eq!(reach(0.0), None);
    }

    /// A unit circle strays from its tangent line at its top by `1 - cos a` at the angle
    /// `a` from it: over a reach of 1e-3 the stray enclosed is at least the 5e-7 of the
    /// circle's points there (the point at 5e-4 rad is 1.25e-7 off, which a stray of zero
    /// left outside a region of radius 1e-8) and within a few times it; a line does not
    /// stray at all, to its rounding; a reach of zero or NaN encloses nothing.
    #[test]
    fn a_curved_edge_s_stray_from_its_tangent_is_enclosed() {
        let k = Kernel::default();
        let circle = ellipse(&k, Point3::new(0.0, 0.0, 0.0), 1.0, 1.0);
        let top = Point3::new(0.0, 1.0, 0.0);
        let s = at(&k, &circle, top);
        let reach = 1e-3;
        let stray = circle
            .stray_from_line(&k, s, Vec3::new(1.0, 0.0, 0.0), top, reach)
            .unwrap()
            .unwrap();
        for a in [5e-4_f64, -5e-4, 0.999e-3, -0.999e-3] {
            let off = 1.0 - a.cos();
            assert!(stray >= off, "stray {stray:e} below {off:e} at {a}");
        }
        assert!(stray < 4.0 * (1.0 - 1e-3_f64.cos()), "{stray:e}");
        let line = Curve::from(Line::from_points(top - Vec3::X, top + Vec3::X, 3));
        let t = at(&k, &line, top);
        let flat = line
            .stray_from_line(&k, t, Vec3::new(1.0, 0.0, 0.0), top, reach)
            .unwrap()
            .unwrap();
        assert!(flat < 1e-13, "{flat:e}");
        for bad in [0.0, f64::NAN] {
            assert_eq!(
                circle
                    .stray_from_line(&k, s, Vec3::new(1.0, 0.0, 0.0), top, bad)
                    .unwrap(),
                None
            );
        }
    }
}
