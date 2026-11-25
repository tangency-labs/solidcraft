//! A certified bound of how far apart two B-spline curves are at the same parameters, and of
//! their Hausdorff distance over two parameter intervals.
//!
//! Upstream: none (not upstream; solidcore-native).
//!
//! | Rust                   | Upstream |
//! |------------------------|----------|
//! | [`certified_distance`] | none     |
//! | [`certified_hausdorff`] | none    |
//! | [`interpolation_bound`] | none    |
//!
//! Both curves are cut into Bézier pieces over the union of their breakpoints (and, for
//! [`certified_hausdorff`], the intervals' ends; [`Curve::refine`], [`Curve::decompose_bez`]) and each piece raised to the higher of the
//! two degrees ([`Curve::elevate_degree`], on a piece with no interior knot only the Bézier
//! elevation). On a common span the difference of two pieces of one degree is the Bézier
//! piece of the differences of their control points, which lies in their convex hull
//! (Farouki and Rajan 1987; Piegl and Tiller, *The NURBS Book*, 2nd ed., §5.6): at no
//! parameter of the span are the curves farther apart than the largest difference.
//!
//! Rounding (docs/definitions-and-certificates.md §4.4): every new control point of a knot
//! insertion is one convex combination of two points, its ratio a quotient of knot
//! differences and its complement one subtraction, so each insertion adds at most
//! `γ_12 M` per coordinate, `M` the net's largest coordinate (a convex combination neither
//! grows the coordinates nor the errors it is given); `I` insertions carry `γ_(12 (I + 1))`
//! of twice `M`. Both curves are first moved into their own frame (minus the first control
//! point of the first; each moved coordinate rounds once, `γ_2` of `M`), so `M` is their size
//! there, not their distance from the origin. The elevation of a piece by `t` forms each point as a convex combination of
//! at most `p + 1` points with coefficients that are quotients of binomials: `γ_(8 q)` of
//! `M`, `q` the degree reached. The norm of each difference is rounded up by `γ_8`, and a
//! coordinate error `e` moves a point by at most `2 e` (`√3 e`).
//!
//! [`interpolation_bound`] bounds how far a curve misses points it was fitted through at
//! their parameters: each point of the curve is evaluated (the basis functions of The NURBS
//! Book's A2.3 at order 0, the general basis whatever the knots, and their sum with the
//! control points: `γ_(12 p + 4)` of `M` per coordinate,
//! the count `eval::collapse` gives de Boor's `p` levels) and its computed distance from the
//! data point rounded up by `γ_8`. Every distance is a scaled norm (the components divided by
//! the largest before squaring), so a difference near the bottom of the floating range is not
//! lost to underflow.
//!
//! Deviations: none (no upstream counterpart). Non-rational 3D curves only, of degree 1 to
//! [`MAX_DEGREE`]; their knot vectors clamped, on one domain for [`certified_distance`].

use crate::error::query::QueryFailure;
use crate::error::{Result, SolidError};
use crate::math::Vec3;
use crate::nurbs::crv_basic::Curve;
use crate::nurbs::knots::{Side, MAX_DEGREE};
use crate::nurbs::math::{gamma, CPoint, NOW, NOZ};

const OP: &str = "nurbs::crv_deviation::certified_distance";
const OP_HAUSDORFF: &str = "nurbs::crv_deviation::certified_hausdorff";
const OP_FIT: &str = "nurbs::crv_deviation::interpolation_bound";

/// A bound `d` with `|c(u_k) - q_k| <= d` for every point `q_k` of `points` and its
/// parameter `u_k` of `params`, every rounding of computing it included (module docs): how
/// far the curve fitted through the points misses them.
///
/// # Errors
///
/// `InvalidInput` for a rational or 2D curve, a parameter count other than the points', or a
/// number not finite;
/// [`QueryFailure::BoundOverflows`] when the bound overflows the floating range; those of the
/// evaluation (a parameter outside the knots).
pub fn interpolation_bound(c: &Curve, params: &[f64], points: &[Vec3]) -> Result<f64> {
    if c.cpts().iter().any(|p| p.w != NOW || p.z == NOZ) {
        return Err(SolidError::invalid_input(OP_FIT, "a rational or 2D curve"));
    }
    if params.len() != points.len() {
        return Err(SolidError::invalid_input(
            OP_FIT,
            "one parameter per point needed",
        ));
    }
    // a NaN distance would be dropped by `f64::max`: every number is checked first
    if !shaped(c) {
        return Err(SolidError::invalid_input(
            OP_FIT,
            "a degree of 0 or past the most, or knots that do not fit the points",
        ));
    }
    if !finite(c)
        || !params.iter().all(|u| u.is_finite())
        || !points
            .iter()
            .all(|q| q.x.is_finite() && q.y.is_finite() && q.z.is_finite())
    {
        return Err(SolidError::invalid_input(OP_FIT, "numbers not finite"));
    }
    let size = largest_coordinate(c);
    let mut largest = 0.0_f64;
    for (&u, q) in params.iter().zip(points) {
        // the general basis (derivatives of order 0): the NLib `eval` keeps upstream's
        // clamped reading of a domain's ends, wrong on a periodic curve's
        let p = c
            .derivs(u, Side::Left, 0)?
            .first()
            .copied()
            .ok_or_else(|| SolidError::query(OP_FIT, QueryFailure::Defect { why: "the curve's evaluation of order 0 returned no point, though an evaluation that succeeds returns the point it is asked for" }))?;
        let d = norm_up(p.x - q.x, p.y - q.y, p.z - q.z)
            .ok_or_else(|| SolidError::query(OP_FIT, QueryFailure::BoundOverflows))?;
        largest = largest.max(d);
    }
    let eval_err = gamma(c.degree().saturating_mul(12).saturating_add(4)) * size;
    let bound = (largest + 2.0 * eval_err) * (1.0 + gamma(2));
    if !bound.is_finite() {
        return Err(SolidError::query(OP_FIT, QueryFailure::BoundOverflows));
    }
    Ok(bound)
}

/// A bound `d` with `|a(t) - b(t)| <= d` at every parameter `t` of the curves' common
/// domain, every rounding of computing it included: the largest distance between the
/// control points of their Bézier pieces raised to one degree over common spans (module
/// docs). A curve and itself, with a knot inserted or its degree raised, are within
/// rounding.
///
/// # Errors
///
/// `InvalidInput` for a rational or 2D curve, a number not finite, knots not clamped, or two
/// curves on different domains; [`QueryFailure::BoundOverflows`] when the bound overflows the
/// floating range; those of the refinement, the extraction and the elevation.
pub fn certified_distance(a: &Curve, b: &Curve) -> Result<f64> {
    check(a, OP)?;
    check(b, OP)?;
    if a.knots().first() != b.knots().first() || a.knots().last() != b.knots().last() {
        return Err(SolidError::invalid_input(
            OP,
            "the curves are not on one domain",
        ));
    }
    let (a_local, b_local) = in_own_frame(a, b, OP)?;
    let (a, b) = (&a_local, &b_local);
    let q = a.degree().max(b.degree());
    let size = largest_coordinate(a).max(largest_coordinate(b));
    let moved = gamma(2) * size;
    let (pa, ea) = pieces(a, b, &[], q, size, OP)?;
    let (pb, eb) = pieces(b, a, &[], q, size, OP)?;
    let (ea, eb) = (ea + moved, eb + moved);
    let largest = largest_gap(&pa, &pb, OP)?;
    let bound = (largest + 2.0 * (ea + eb)) * (1.0 + gamma(2));
    if !bound.is_finite() {
        return Err(SolidError::query(OP, QueryFailure::BoundOverflows));
    }
    Ok(bound)
}

/// A bound `d` such that every point of `a` over `ia` is within `d` of a point of `b` over
/// `ib` and every point of `b` over `ib` within `d` of a point of `a` over `ia` (their
/// Hausdorff distance), every rounding of computing it included. The intervals need not be
/// equal, nor the curves' domains: two edges whose ends differ in the last bits are bounded.
///
/// Over the parameters both intervals hold, `[c0, c1]`, the bound is [`certified_distance`]'s,
/// taken on the Bézier pieces of that common interval only (each curve is also cut at both
/// intervals' ends). A piece of one interval past the common one (`[ia_0, c0]`, say) lies in
/// the convex hull of its control points, so each of its points is within the largest
/// distance `s` of those from the curve's point at `c0`, which is within the common bound of
/// the other curve's point at `c0`, a point of the other edge: the bound is the common one
/// plus the largest such `s`. Each of the two points of an `s` carries the curve's coordinate
/// error (`4 e` more), and the four additions, three deep, and the product round (`γ_5`).
/// Without such a piece (equal intervals) the bound is the common one alone, as
/// [`certified_distance`] forms it: on the curves' whole common domain, the same number.
///
/// # Errors
///
/// Those of [`certified_distance`] but for the domains; `InvalidInput` for an interval with a
/// number not finite, reversed, or not inside its curve's domain, and for intervals that share
/// no more than a point.
pub fn certified_hausdorff(a: &Curve, ia: [f64; 2], b: &Curve, ib: [f64; 2]) -> Result<f64> {
    check(a, OP_HAUSDORFF)?;
    check(b, OP_HAUSDORFF)?;
    let inside = |c: &Curve, [lo, hi]: [f64; 2]| {
        lo.is_finite()
            && hi.is_finite()
            && lo <= hi
            && c.knots().first().is_some_and(|&k| k <= lo)
            && c.knots().last().is_some_and(|&k| hi <= k)
    };
    if !(inside(a, ia) && inside(b, ib)) {
        return Err(SolidError::invalid_input(
            OP_HAUSDORFF,
            "an interval not inside its curve's domain",
        ));
    }
    let (c0, c1) = (ia[0].max(ib[0]), ia[1].min(ib[1]));
    if c0 >= c1 {
        return Err(SolidError::invalid_input(
            OP_HAUSDORFF,
            "the intervals do not overlap",
        ));
    }
    let (a_local, b_local) = in_own_frame(a, b, OP_HAUSDORFF)?;
    let (a, b) = (&a_local, &b_local);
    let q = a.degree().max(b.degree());
    let size = largest_coordinate(a).max(largest_coordinate(b));
    let moved = gamma(2) * size;
    let ends = [ia[0], ia[1], ib[0], ib[1]];
    let (pa, ea) = pieces(a, b, &ends, q, size, OP_HAUSDORFF)?;
    let (pb, eb) = pieces(b, a, &ends, q, size, OP_HAUSDORFF)?;
    let (ea, eb) = (ea + moved, eb + moved);
    // the pieces over [lo, hi]; every interval end is a breakpoint of both curves' pieces
    let over = |p: &[Curve], lo: f64, hi: f64| -> Vec<Curve> {
        p.iter()
            .filter(|x| {
                x.knots().first().is_some_and(|&k| lo <= k)
                    && x.knots().last().is_some_and(|&k| k <= hi)
            })
            .cloned()
            .collect()
    };
    let (ca, cb) = (over(&pa, c0, c1), over(&pb, c0, c1));
    if ca.is_empty() {
        return Err(SolidError::query(OP_HAUSDORFF, QueryFailure::Defect { why: "every interval end is a breakpoint of both curves' pieces and the intervals overlap past a point, so a piece lies in their common interval" }));
    }
    let largest = largest_gap(&ca, &cb, OP_HAUSDORFF)?;
    // the largest distance of a piece past the common interval from the curve's point there,
    // `None` without such a piece
    let beyond = |p: &[Curve], common: &[Curve], [lo, hi]: [f64; 2]| -> Result<Option<f64>> {
        let first = common.first().and_then(|x| x.cpts().first());
        let last = common.last().and_then(|x| x.cpts().last());
        let (Some(&at_c0), Some(&at_c1)) = (first, last) else {
            return Err(SolidError::query(OP_HAUSDORFF, QueryFailure::Defect { why: "the common pieces were checked to pair up, and a Bézier piece of degree 1 or more has at least two control points" }));
        };
        let mut s = None;
        for (lo, hi, end) in [(lo, c0, at_c0), (c1, hi, at_c1)] {
            if lo >= hi {
                continue;
            }
            for r in over(p, lo, hi).iter().flat_map(Curve::cpts) {
                let d = norm_up(r.x - end.x, r.y - end.y, r.z - end.z)
                    .ok_or_else(|| SolidError::query(OP_HAUSDORFF, QueryFailure::BoundOverflows))?;
                s = Some(s.map_or(d, |m: f64| m.max(d)));
            }
        }
        Ok(s)
    };
    let bound = match (beyond(&pa, &ca, ia)?, beyond(&pb, &cb, ib)?) {
        (None, None) => (largest + 2.0 * (ea + eb)) * (1.0 + gamma(2)),
        (sa, sb) => {
            let sliver = sa.unwrap_or(0.0).max(sb.unwrap_or(0.0));
            let err = 2.0 * (ea + eb) + 4.0 * ea.max(eb);
            (largest + sliver + err) * (1.0 + gamma(5))
        }
    };
    if !bound.is_finite() {
        return Err(SolidError::query(
            OP_HAUSDORFF,
            QueryFailure::BoundOverflows,
        ));
    }
    Ok(bound)
}

/// `a` and `b` in their own frame: both moved by minus `a`'s first control point, so the
/// rounding counted on them follows their size, not their distance from the origin. The
/// difference of the two is the same in either frame; each moved coordinate rounds once.
fn in_own_frame(a: &Curve, b: &Curve, op: &'static str) -> Result<(Curve, Curve)> {
    let Some(&c0) = a.cpts().first() else {
        return Err(SolidError::invalid_input(op, "a curve without points"));
    };
    let local = |c: &Curve| -> Result<Curve> {
        let pw = c
            .cpts()
            .iter()
            .map(|p| CPoint::new(p.x - c0.x, p.y - c0.y, p.z - c0.z, NOW))
            .collect();
        Curve::new(pw, c.degree(), c.knots().to_vec())
    };
    Ok((local(a)?, local(b)?))
}

/// The largest distance between the control points of two lists of Bézier pieces of one
/// degree on the same spans (computed, each rounded up by `γ_8`).
fn largest_gap(pa: &[Curve], pb: &[Curve], op: &'static str) -> Result<f64> {
    if pa.len() != pb.len() {
        return Err(SolidError::query(op, QueryFailure::Defect { why: "both curves are cut into Bézier pieces at the union of their breakpoints, so their pieces pair up span by span" }));
    }
    let mut largest = 0.0_f64;
    for (x, y) in pa.iter().zip(pb) {
        if x.knots().first() != y.knots().first() || x.knots().last() != y.knots().last() {
            return Err(SolidError::query(op, QueryFailure::Defect { why: "both curves are cut into Bézier pieces at the union of their breakpoints, so their pieces pair up span by span" }));
        }
        if x.cpts().len() != y.cpts().len() {
            return Err(SolidError::query(op, QueryFailure::Defect { why: "both curves' pieces are raised to the higher of the two degrees, so paired pieces have as many control points" }));
        }
        for (p, r) in x.cpts().iter().zip(y.cpts()) {
            let d = norm_up(p.x - r.x, p.y - r.y, p.z - r.z)
                .ok_or_else(|| SolidError::query(op, QueryFailure::BoundOverflows))?;
            largest = largest.max(d);
        }
    }
    Ok(largest)
}

/// An upper bound of the length of the difference `(dx, dy, dz)` (each one subtraction),
/// every rounding included and nothing lost to underflow: the components are divided by the
/// largest before they are squared (each quotient rounds by `γ_1`, the squares, sums and
/// root by `γ_5`, the product by `γ_1`, the subtractions by `γ_1`: `γ_8` in all), and the
/// smallest subnormals a product with a subnormal largest can lose are added back. `None`
/// for a difference that overflows.
pub(crate) fn norm_up(dx: f64, dy: f64, dz: f64) -> Option<f64> {
    let s = dx.abs().max(dy.abs()).max(dz.abs());
    if !s.is_finite() {
        return None;
    }
    if s == 0.0 {
        return Some(0.0);
    }
    let (x, y, z) = (dx / s, dy / s, dz / s);
    let bound = s * (x * x + y * y + z * z).sqrt() * (1.0 + gamma(8)) + 4.0 * f64::from_bits(1);
    bound.is_finite().then_some(bound)
}

/// Whether `c`'s degree is one NLib takes (1 to [`MAX_DEGREE`]) and its knots fit its control
/// points (their count plus the degree plus one), checked before any arithmetic on the
/// degree.
fn shaped(c: &Curve) -> bool {
    let p = c.degree();
    (1..=MAX_DEGREE).contains(&p) && c.knots().len() == c.cpts().len() + p + 1
}

/// Whether every control point coordinate and knot of `c` is finite.
fn finite(c: &Curve) -> bool {
    c.cpts()
        .iter()
        .all(|p| p.x.is_finite() && p.y.is_finite() && p.z.is_finite())
        && c.knots().iter().all(|k| k.is_finite())
}

/// Refuses a curve the bound does not take: rational, 2D, with a number not finite, or with
/// knots not clamped.
fn check(c: &Curve, op: &'static str) -> Result<()> {
    if !finite(c) {
        return Err(SolidError::invalid_input(op, "numbers not finite"));
    }
    if !shaped(c) {
        return Err(SolidError::invalid_input(
            op,
            "a degree of 0 or past the most, or knots that do not fit the points",
        ));
    }
    if c.cpts().iter().any(|p| p.w != NOW) {
        return Err(SolidError::invalid_input(op, "a rational curve"));
    }
    if c.cpts().iter().any(|p| p.z == NOZ) {
        return Err(SolidError::invalid_input(op, "a 2D curve"));
    }
    let p = c.degree();
    let knots = c.knots();
    let flat =
        |run: Option<&[f64]>| run.is_some_and(|r| r.windows(2).all(|w| w.first() == w.get(1)));
    let last = knots.len().checked_sub(p + 1);
    if p == 0 || !flat(knots.get(..=p)) || !flat(last.and_then(|s| knots.get(s..))) {
        return Err(SolidError::invalid_input(op, "knots not clamped"));
    }
    Ok(())
}

/// The largest absolute coordinate of `c`'s control points.
fn largest_coordinate(c: &Curve) -> f64 {
    c.cpts().iter().fold(0.0_f64, |m, p| {
        m.max(p.x.abs()).max(p.y.abs()).max(p.z.abs())
    })
}

/// The Bézier pieces of `c` over the union of its breakpoints, `other`'s and `ends`, each
/// raised to degree `q`, and the error each of their coordinates may carry (module docs),
/// `size` the largest coordinate of both nets.
fn pieces(
    c: &Curve,
    other: &Curve,
    ends: &[f64],
    q: usize,
    size: f64,
    op: &'static str,
) -> Result<(Vec<Curve>, f64)> {
    let (Some(&lo), Some(&hi)) = (c.knots().first(), c.knots().last()) else {
        return Err(SolidError::invalid_input(op, "a curve without knots"));
    };
    // the breakpoints of `other` and the `ends` that `c` lacks, once each, in order
    let mut missing: Vec<f64> = other
        .knots()
        .iter()
        .chain(ends)
        .copied()
        .filter(|&u| u > lo && u < hi && !c.knots().contains(&u))
        .collect();
    missing.sort_by(f64::total_cmp);
    missing.dedup();
    let refined = if missing.is_empty() {
        c.clone()
    } else {
        c.refine(&missing)?
    };
    let p = c.degree();
    let pieces = refined.decompose_bez()?;
    // knots inserted: those of the pieces' joined knot vector, 2 (p + 1) + (s - 1) p, past c's
    let joined = pieces
        .len()
        .saturating_sub(1)
        .saturating_mul(p)
        .saturating_add(2 * (p + 1));
    let inserted = joined.saturating_sub(c.knots().len());
    let mut err = gamma(inserted.saturating_add(1).saturating_mul(12)) * 2.0 * size;
    let raise = q.saturating_sub(p);
    if raise == 0 {
        return Ok((pieces, err));
    }
    err += gamma(q.saturating_mul(8)) * (size + err);
    let raised = pieces
        .iter()
        .map(|piece| piece.elevate_degree(raise))
        .collect::<Result<Vec<_>>>()?;
    Ok((raised, err))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nurbs::knots::clamped_uniform;

    fn spline(points: &[[f64; 3]], degree: usize) -> Curve {
        let pw = points
            .iter()
            .map(|&[x, y, z]| CPoint::from_point(Vec3::new(x, y, z)))
            .collect();
        Curve::new(pw, degree, clamped_uniform(points.len(), degree).unwrap()).unwrap()
    }

    const WAVE: [[f64; 3]; 6] = [
        [0.0, 0.0, 0.0],
        [1.0, 2.0, 0.0],
        [2.0, -1.0, 0.0],
        [3.0, 3.0, 0.0],
        [4.0, 0.0, 1.0],
        [5.0, 1.0, 0.0],
    ];

    #[test]
    fn a_curve_is_no_distance_from_itself_or_its_refinements() {
        let c = spline(&WAVE, 3);
        let d = certified_distance(&c, &c).unwrap();
        assert!(d < 1e-12, "{d}");
        let more = c.insert_knot(0.3, 2).unwrap();
        let higher = c.elevate_degree(2).unwrap();
        for other in [&more, &higher] {
            let d = certified_distance(&c, other).unwrap();
            assert!(d < 1e-12, "{d}");
            let d = certified_distance(other, &c).unwrap();
            assert!(d < 1e-12, "{d}");
        }
    }

    #[test]
    fn a_moved_control_point_is_bounded_by_its_move_and_not_less_than_the_curves_gap() {
        let c = spline(&WAVE, 3);
        let mut moved = WAVE;
        moved[2][2] += 1.0;
        let m = spline(&moved, 3);
        let d = certified_distance(&c, &m).unwrap();
        // the gap is the moved point's basis function times 1: at most 1, and the bound
        // holds every sampled gap
        assert!(d <= 1.0 + 1e-12, "{d}");
        for i in 0..=200 {
            let t = f64::from(i) / 200.0;
            let (p, r) = (
                c.eval(t, Side::Left).unwrap(),
                m.eval(t, Side::Left).unwrap(),
            );
            let gap2 = (p.x - r.x).powi(2) + (p.y - r.y).powi(2) + (p.z - r.z).powi(2);
            assert!(gap2 <= d * d, "at {t}: {gap2} past {d}²");
        }
    }

    #[test]
    fn curves_of_other_degrees_and_knots_are_compared_on_common_pieces() {
        // a line from 0 to 5 along x at speed 5, as a polyline of six points (knots at
        // fifths) and as one segment, and that segment raised to a cubic
        let polyline = spline(&[0.0, 1.0, 2.0, 3.0, 4.0, 5.0].map(|x| [x, 0.0, 0.0]), 1);
        let line = spline(&[[0.0, 0.0, 0.0], [5.0, 0.0, 0.0]], 1);
        let d = certified_distance(&polyline, &line).unwrap();
        assert!(d < 1e-12, "{d}");
        let cubic = line.elevate_degree(2).unwrap();
        let d = certified_distance(&polyline, &cubic).unwrap();
        assert!(d < 1e-12, "{d}");
        // moved off the line by 2 in y at its middle: at least the gap there
        let bent = spline(&[[0.0, 0.0, 0.0], [2.5, 4.0, 0.0], [5.0, 0.0, 0.0]], 2);
        let d = certified_distance(&polyline, &bent).unwrap();
        assert!(d >= 2.0, "{d}");
    }

    #[test]
    fn a_periodic_curves_ends_are_read_through_the_general_basis() {
        // the periodic uniform cubic on a square's corners (polygon wrapped, knots unclamped)
        // starts and ends at (P0 + 4 P1 + P2) / 6; the NLib eval would answer P0 and P2
        let square = [
            [0.0, 0.0, 0.0],
            [10.0, 0.0, 0.0],
            [10.0, 10.0, 0.0],
            [0.0, 10.0, 0.0],
        ];
        let pw = square
            .iter()
            .chain(&square[..3])
            .map(|&[x, y, z]| CPoint::from_point(Vec3::new(x, y, z)))
            .collect();
        let knots = crate::nurbs::knots::periodic_uniform(4, 3).unwrap();
        let closed = Curve::new(pw, 3, knots).unwrap();
        let start = Vec3::new(50.0 / 6.0, 10.0 / 6.0, 0.0);
        let d = interpolation_bound(&closed, &[0.0, 1.0], &[start, start]).unwrap();
        assert!(d < 1e-12, "{d}");
    }

    #[test]
    fn an_interpolants_bound_holds_its_misses_and_counts_them() {
        let c = spline(&WAVE, 3);
        let params = [0.0, 0.25, 0.5, 1.0];
        let on: Vec<Vec3> = params
            .iter()
            .map(|&u| c.eval(u, Side::Left).unwrap())
            .collect();
        let d = interpolation_bound(&c, &params, &on).unwrap();
        assert!(d < 1e-12, "{d}");
        let mut off = on.clone();
        off[2].z += 0.5;
        let d = interpolation_bound(&c, &params, &off).unwrap();
        assert!((0.5..0.5 + 1e-12).contains(&d), "{d}");
        assert!(interpolation_bound(&c, &params[..3], &on).is_err());
    }

    #[test]
    fn numbers_not_finite_are_refused_not_dropped() {
        let c = spline(&WAVE, 3);
        let params = [0.0, 0.5, 1.0];
        let mut on: Vec<Vec3> = params
            .iter()
            .map(|&u| c.eval(u, Side::Left).unwrap())
            .collect();
        assert!(matches!(
            interpolation_bound(&c, &[0.0, f64::NAN, 1.0], &on),
            Err(SolidError::InvalidInput { .. })
        ));
        on[1].y = f64::NAN;
        assert!(matches!(
            interpolation_bound(&c, &params, &on),
            Err(SolidError::InvalidInput { .. })
        ));
        let mut bad = WAVE;
        bad[2][0] = f64::NAN;
        let nan = spline(&bad, 3);
        assert!(matches!(
            certified_distance(&c, &nan),
            Err(SolidError::InvalidInput { .. })
        ));
        assert!(matches!(
            interpolation_bound(&nan, &params, &on[..]),
            Err(SolidError::InvalidInput { .. })
        ));
    }

    /// Finite curves too far apart for the bound: each difference of control points is
    /// finite, its length is not.
    #[test]
    fn curves_too_far_apart_for_the_bound_overflow_by_name() {
        use crate::error::query::QueryFailure;
        let a = spline(&[[8e307, 8e307, 0.0], [8e307, 8e307, 1.0]], 1);
        let b = spline(&[[-8e307, -8e307, 0.0], [-8e307, -8e307, 1.0]], 1);
        for e in [
            certified_distance(&a, &b).unwrap_err(),
            certified_hausdorff(&a, [0.0, 1.0], &b, [0.0, 1.0]).unwrap_err(),
        ] {
            assert_eq!(
                e.query_failure(),
                Some(&QueryFailure::BoundOverflows),
                "{e}"
            );
            assert_eq!(e.sm_status(), Some(crate::error::SM_ERR));
            assert!(!e.is_defect());
        }
    }

    #[test]
    fn an_extreme_degree_is_refused_not_overflowed() {
        let c = spline(&WAVE, 3);
        let pw = WAVE
            .iter()
            .map(|&[x, y, z]| CPoint::from_point(Vec3::new(x, y, z)))
            .collect();
        let wild = Curve::new(pw, usize::MAX, clamped_uniform(6, 3).unwrap()).unwrap();
        assert!(matches!(
            certified_distance(&c, &wild),
            Err(SolidError::InvalidInput { .. })
        ));
        assert!(matches!(
            certified_distance(&wild, &c),
            Err(SolidError::InvalidInput { .. })
        ));
        assert!(matches!(
            interpolation_bound(&wild, &[0.0], &[Vec3::new(0.0, 0.0, 0.0)]),
            Err(SolidError::InvalidInput { .. })
        ));
    }

    #[test]
    fn a_gap_near_the_bottom_of_the_floating_range_is_not_lost_to_underflow() {
        // a line of length 1e-200 and points 1e-200 off it: squaring 1e-200 underflows to 0
        let line = spline(&[[0.0, 0.0, 0.0], [1e-200, 0.0, 0.0]], 1);
        let off = Vec3::new(0.0, 1e-200, 0.0);
        let d = interpolation_bound(&line, &[0.0], &[off]).unwrap();
        assert!(d >= 1e-200, "{d}");
        let beside = spline(&[[0.0, 1e-200, 0.0], [1e-200, 1e-200, 0.0]], 1);
        let d = certified_distance(&line, &beside).unwrap();
        assert!(d >= 1e-200, "{d}");
        assert!(d <= 1.0001e-200, "{d}");
    }

    #[test]
    fn the_bound_follows_the_curves_size_not_their_place() {
        // the wave a thousand kilometres along x, against itself and its refinement: the
        // rounding counted is the wave's, not 1e9's
        let far: Vec<[f64; 3]> = WAVE.iter().map(|&[x, y, z]| [x + 1e9, y, z]).collect();
        let c = spline(&far, 3);
        let d = certified_distance(&c, &c).unwrap();
        assert!(d < 1e-9, "{d}");
        let higher = c.elevate_degree(1).unwrap();
        let d = certified_distance(&c, &higher).unwrap();
        // the raise itself rounds at 1e9 (u 1e9 is about 1e-7): the bound holds that, no more
        assert!(d < 1e-5, "{d}");
    }

    /// The proof's path, sampled: with the arguments in either order, each curve's point at
    /// `t` is within the bound of the other curve's point at `t` held to the other's interval.
    /// Returns the larger of the two bounds.
    fn held_both_ways(a: &Curve, ia: [f64; 2], b: &Curve, ib: [f64; 2]) -> f64 {
        let bounds = [
            certified_hausdorff(a, ia, b, ib).unwrap(),
            certified_hausdorff(b, ib, a, ia).unwrap(),
        ];
        for d in bounds {
            for (x, ix, y, iy) in [(a, ia, b, ib), (b, ib, a, ia)] {
                for i in 0..=400 {
                    let t = ix[0] + (ix[1] - ix[0]) * f64::from(i) / 400.0;
                    let p = x.eval(t, Side::Left).unwrap();
                    let r = y.eval(t.clamp(iy[0], iy[1]), Side::Left).unwrap();
                    assert!(p.distance(r) <= d, "at {t}: {} past {d}", p.distance(r));
                }
            }
        }
        bounds[0].max(bounds[1])
    }

    /// Over intervals that differ, a piece past the common interval is bounded from the
    /// curve's point at the common end, at either end and for either argument. The wave
    /// against itself on a domain ending a few bits short is within rounding; over halves of
    /// its domain against the whole, the bound holds every sampled point and is at least the
    /// far end's distance. A curve that runs off and comes back near the common end is
    /// bounded by how far it runs, not by its end. A quadratic against its cubic elevation
    /// compares raised pieces. Intervals that do not overlap, are reversed or not finite, or
    /// reach past the domain, are refused.
    #[test]
    fn intervals_that_differ_are_bounded_through_the_common_ends() {
        let c = spline(&WAVE, 3);
        let mut knots = c.knots().to_vec();
        let short = 1.0 - 4.0 * f64::EPSILON;
        for k in knots.iter_mut().filter(|k| **k == 1.0) {
            *k = short;
        }
        let rounded = Curve::new(c.cpts().to_vec(), 3, knots).unwrap();
        let d = held_both_ways(&c, [0.0, 1.0], &rounded, [0.0, short]);
        assert!(d < 1e-12, "{d}");
        let at = |t: f64| c.eval(t, Side::Left).unwrap();
        for (part, far, common) in [([0.0, 0.5], 1.0, 0.5), ([0.5, 1.0], 0.0, 0.5)] {
            let d = held_both_ways(&c, [0.0, 1.0], &c, part);
            assert!(d >= at(far).distance(at(common)), "{d}");
        }
        // a polyline through its points at quarters: past the middle out to x = 6 and back to
        // 0.1, its far end 0.1 from its middle
        let back = spline(
            &[
                [-2.0, 0.0, 0.0],
                [-1.0, 0.0, 0.0],
                [0.0, 0.0, 0.0],
                [6.0, 0.0, 0.0],
                [0.1, 0.0, 0.0],
            ],
            1,
        );
        let ends = back
            .eval(1.0, Side::Left)
            .unwrap()
            .distance(back.eval(0.5, Side::Left).unwrap());
        let d = held_both_ways(&back, [0.0, 1.0], &back, [0.0, 0.5]);
        assert!(d > 2.0 * ends, "{d} against the ends' {ends}");
        let quadratic = spline(&WAVE, 2);
        let cubic = quadratic.elevate_degree(1).unwrap();
        let d = held_both_ways(&quadratic, [0.0, 1.0], &cubic, [0.0, 1.0]);
        assert!(d < 1e-12, "{d}");
        held_both_ways(&quadratic, [0.0, 1.0], &cubic, [0.25, 0.75]);
        for (ia, ib) in [
            ([0.0, 0.5], [0.5, 1.0]),
            ([0.0, 1.0], [0.5, 1.5]),
            ([0.6, 0.4], [0.0, 1.0]),
            ([0.0, f64::NAN], [0.0, 1.0]),
        ] {
            assert!(matches!(
                certified_hausdorff(&c, ia, &c, ib),
                Err(SolidError::InvalidInput { .. })
            ));
        }
    }

    #[test]
    fn rational_or_other_domains_are_refused() {
        let c = spline(&WAVE, 3);
        let mut other = c.clone();
        other
            .reparam_to_interval(crate::nurbs::geom::Interval { ul: 0.0, ur: 2.0 })
            .unwrap();
        assert!(matches!(
            certified_distance(&c, &other),
            Err(SolidError::InvalidInput { .. })
        ));
        let mut rational = c.clone();
        rational.non_rat_to_rat();
        assert!(matches!(
            certified_distance(&c, &rational),
            Err(SolidError::InvalidInput { .. })
        ));
    }
}
