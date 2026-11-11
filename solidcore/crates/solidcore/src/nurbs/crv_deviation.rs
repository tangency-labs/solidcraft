//! A certified bound of how far apart two B-spline curves are at the same parameters.
//!
//! Upstream: none (not upstream; solidcore-native).
//!
//! | Rust                   | Upstream |
//! |------------------------|----------|
//! | [`certified_distance`] | none     |
//! | [`interpolation_bound`] | none    |
//!
//! Both curves are cut into Bézier pieces over the union of their breakpoints
//! ([`Curve::refine`], [`Curve::decompose_bez`]) and each piece raised to the higher of the
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
//! [`MAX_DEGREE`]; their knot vectors clamped on one domain.

use crate::error::{Result, SolidError};
use crate::math::Vec3;
use crate::nurbs::crv_basic::Curve;
use crate::nurbs::knots::{Side, MAX_DEGREE};
use crate::nurbs::math::{gamma, CPoint, NOW, NOZ};

const OP: &str = "nurbs::crv_deviation::certified_distance";
const OP_FIT: &str = "nurbs::crv_deviation::interpolation_bound";

/// A bound `d` with `|c(u_k) - q_k| <= d` for every point `q_k` of `points` and its
/// parameter `u_k` of `params`, every rounding of computing it included (module docs): how
/// far the curve fitted through the points misses them.
///
/// # Errors
///
/// `InvalidInput` for a rational or 2D curve, a parameter count other than the points', or a
/// number not finite;
/// a query failure when the bound is not finite; those of the evaluation (a parameter
/// outside the knots).
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
            .ok_or_else(|| SolidError::query_failed(OP_FIT, "no point evaluated"))?;
        let d = norm_up(p.x - q.x, p.y - q.y, p.z - q.z)
            .ok_or_else(|| SolidError::query_failed(OP_FIT, "the bound is not finite"))?;
        largest = largest.max(d);
    }
    let eval_err = gamma(c.degree().saturating_mul(12).saturating_add(4)) * size;
    let bound = (largest + 2.0 * eval_err) * (1.0 + gamma(2));
    if !bound.is_finite() {
        return Err(SolidError::query_failed(OP_FIT, "the bound is not finite"));
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
/// curves on different domains; a query failure when their pieces do not share their spans or the bound is not
/// finite; those of the refinement, the extraction and the elevation.
pub fn certified_distance(a: &Curve, b: &Curve) -> Result<f64> {
    check(a)?;
    check(b)?;
    if a.knots().first() != b.knots().first() || a.knots().last() != b.knots().last() {
        return Err(SolidError::invalid_input(
            OP,
            "the curves are not on one domain",
        ));
    }
    // In the curves' own frame: both moved by minus `a`'s first control point, so the
    // rounding counted below follows their size, not their distance from the origin. The
    // difference of the two is the same in either frame; each moved coordinate rounds once.
    let Some(&c0) = a.cpts().first() else {
        return Err(SolidError::invalid_input(OP, "a curve without points"));
    };
    let local = |c: &Curve| -> Result<Curve> {
        let pw = c
            .cpts()
            .iter()
            .map(|p| CPoint::new(p.x - c0.x, p.y - c0.y, p.z - c0.z, NOW))
            .collect();
        Curve::new(pw, c.degree(), c.knots().to_vec())
    };
    let (a_local, b_local) = (local(a)?, local(b)?);
    let (a, b) = (&a_local, &b_local);
    let q = a.degree().max(b.degree());
    let size = largest_coordinate(a).max(largest_coordinate(b));
    let moved = gamma(2) * size;
    let (pa, ea) = pieces(a, b, q, size)?;
    let (pb, eb) = pieces(b, a, q, size)?;
    let (ea, eb) = (ea + moved, eb + moved);
    if pa.len() != pb.len() {
        return Err(SolidError::query_failed(
            OP,
            "the curves' pieces do not share their spans",
        ));
    }
    let mut largest = 0.0_f64;
    for (x, y) in pa.iter().zip(&pb) {
        if x.knots().first() != y.knots().first() || x.knots().last() != y.knots().last() {
            return Err(SolidError::query_failed(
                OP,
                "the curves' pieces do not share their spans",
            ));
        }
        if x.cpts().len() != y.cpts().len() {
            return Err(SolidError::query_failed(
                OP,
                "the curves' pieces are not of one degree",
            ));
        }
        for (p, r) in x.cpts().iter().zip(y.cpts()) {
            let d = norm_up(p.x - r.x, p.y - r.y, p.z - r.z)
                .ok_or_else(|| SolidError::query_failed(OP, "the bound is not finite"))?;
            largest = largest.max(d);
        }
    }
    let bound = (largest + 2.0 * (ea + eb)) * (1.0 + gamma(2));
    if !bound.is_finite() {
        return Err(SolidError::query_failed(OP, "the bound is not finite"));
    }
    Ok(bound)
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
fn check(c: &Curve) -> Result<()> {
    if !finite(c) {
        return Err(SolidError::invalid_input(OP, "numbers not finite"));
    }
    if !shaped(c) {
        return Err(SolidError::invalid_input(
            OP,
            "a degree of 0 or past the most, or knots that do not fit the points",
        ));
    }
    if c.cpts().iter().any(|p| p.w != NOW) {
        return Err(SolidError::invalid_input(OP, "a rational curve"));
    }
    if c.cpts().iter().any(|p| p.z == NOZ) {
        return Err(SolidError::invalid_input(OP, "a 2D curve"));
    }
    let p = c.degree();
    let knots = c.knots();
    let flat =
        |run: Option<&[f64]>| run.is_some_and(|r| r.windows(2).all(|w| w.first() == w.get(1)));
    let last = knots.len().checked_sub(p + 1);
    if p == 0 || !flat(knots.get(..=p)) || !flat(last.and_then(|s| knots.get(s..))) {
        return Err(SolidError::invalid_input(OP, "knots not clamped"));
    }
    Ok(())
}

/// The largest absolute coordinate of `c`'s control points.
fn largest_coordinate(c: &Curve) -> f64 {
    c.cpts().iter().fold(0.0_f64, |m, p| {
        m.max(p.x.abs()).max(p.y.abs()).max(p.z.abs())
    })
}

/// The Bézier pieces of `c` over the union of its breakpoints and `other`'s, each raised to
/// degree `q`, and the error each of their coordinates may carry (module docs), `size` the
/// largest coordinate of both nets.
fn pieces(c: &Curve, other: &Curve, q: usize, size: f64) -> Result<(Vec<Curve>, f64)> {
    let (Some(&lo), Some(&hi)) = (c.knots().first(), c.knots().last()) else {
        return Err(SolidError::invalid_input(OP, "a curve without knots"));
    };
    // the breakpoints of `other` that `c` lacks, once each, in order
    let mut missing: Vec<f64> = Vec::new();
    for &u in other.knots() {
        if u > lo && u < hi && !c.knots().contains(&u) && missing.last() != Some(&u) {
            missing.push(u);
        }
    }
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
