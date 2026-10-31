//! Rational B-spline curves that are valid by construction, and their exact Bézier pieces
//! (#9355). 2D curves are curves with z = 0.
//!
//! A [`NurbsCurve`] is only made by [`NurbsCurve::from_homogeneous`], which checks everything that
//! ties its parts together: the degree, `n + degree + 1` knots for `n` control points, knots that
//! never decrease, a non-empty domain, finite points no further than [`MAX_COORDINATE`] from the
//! origin, positive weights. Arithmetic on a valid curve can still overflow or run out of
//! precision, so its operations return [`SplineResult`].
//!
//! Every piece carries intervals that hold its exact control points ([`super::interval`]): the
//! curve's own, then every knot insertion done in intervals, each factor from knot differences.
//! So what the pieces stand for is exactly the curve as given, whatever rounding happened on the
//! way; their `f64` control points are numbers inside those intervals.

use super::interval::{HInterval, Interval};
use super::{Blend, HPoint, SplineError, SplineResult, check_knots, clamp_row, invalid};

/// Highest degree a curve may have: above the sketch's 9 ([`crate::MAX_BEZIER_DEGREE`]) for the
/// hidden-line views of imported curves. It bounds the work a hostile curve can ask for.
pub const MAX_CURVE_DEGREE: usize = 16;
/// Furthest a control point may be from the origin along any axis (mm).
const MAX_COORDINATE: f64 = 1e12;

/// A rational B-spline curve of `degree` with `knots` and homogeneous control points, checked when
/// it is made (module documentation).
#[derive(Clone, Debug, PartialEq)]
pub struct NurbsCurve {
    degree: usize,
    knots: Vec<f64>,
    exact: Vec<HInterval>,
}

impl NurbsCurve {
    /// The curve over homogeneous control points `(x·w, y·w, z·w, w)`.
    pub fn from_homogeneous(degree: usize, knots: Vec<f64>, ctrl: &[HPoint]) -> SplineResult<NurbsCurve> {
        let (n, p) = (ctrl.len(), degree);
        if !(1..=MAX_CURVE_DEGREE).contains(&p) {
            return Err(SplineError::Unsupported(format!("degree {p} (1 to {MAX_CURVE_DEGREE})")));
        }
        if n <= p || knots.len() != n + p + 1 {
            return invalid(format!("{n} control points of degree {p} need {} knots, not {}", n + p + 1, knots.len()));
        }
        check_knots(&knots)?;
        let (Some(&lo), Some(&hi)) = (knots.get(p), knots.get(n)) else { return invalid("too few knots") };
        if hi <= lo {
            return invalid("the curve's domain is empty");
        }
        for h in ctrl {
            if !(h.w.is_finite() && h.w > 0.0) {
                return invalid("weights must be positive numbers");
            }
            let q = h.project().ok_or_else(|| SplineError::Invalid("a control point is not finite".into()))?;
            if q.x.abs().max(q.y.abs()).max(q.z.abs()) > MAX_COORDINATE {
                return Err(SplineError::TooLarge(format!("a control point more than {MAX_COORDINATE} mm from the origin")));
            }
        }
        let exact = ctrl.iter().map(|h| HInterval::point(*h)).collect();
        Ok(NurbsCurve { degree: p, knots, exact })
    }

    /// The curve's Bézier pieces: one per span of non-zero length of its domain, in order. The
    /// curve is clamped on its domain, then split in one pass (NURBS Book A5.6), in work linear in
    /// its length, all in intervals. An inner knot repeated more than the degree (a curve broken
    /// there) is refused.
    pub fn pieces(&self) -> SplineResult<Vec<CurvePiece>> {
        let p = self.degree;
        let (u, pw) = clamp_row(p, &self.knots, &self.exact)?;
        let bad = || SplineError::Invalid("Bézier decomposition".into());
        let kn = |i: usize| u.get(i).copied().ok_or_else(bad);
        let m = u.len().checked_sub(1).ok_or_else(bad)?;
        let mut piece = pw.get(..=p).ok_or_else(bad)?.to_vec();
        let mut factors = vec![(Interval::ZERO, Interval::ZERO); p];
        let mut out = Vec::new();
        // `a` is the last index of the knot that starts the piece, `b` walks to the one that ends it.
        let (mut a, mut b) = (p, p + 1);
        while b < m {
            let first = b;
            while b < m && kn(b + 1)? == kn(b)? {
                b += 1;
            }
            let mult = b - first + 1;
            let end = b >= m;
            if !end && mult > p {
                return invalid(format!("the inner knot {} repeats more than the degree", kn(b)?));
            }
            let (ua, ub) = (kn(a)?, kn(b)?);
            let mut next = vec![HInterval::ZERO; p + 1];
            if mult < p {
                // Insert `ub` until it repeats `p` times; the piece's points become Bézier points,
                // and the next piece's first ones fall out on the way.
                for j in (mult + 1..=p).rev() {
                    *factors.get_mut(j - mult - 1).ok_or_else(bad)? = HInterval::factors(ua, ub, kn(a + j)?);
                }
                let r = p - mult;
                for j in 1..=r {
                    let s = mult + j;
                    for k in (s..=p).rev() {
                        let (alpha, beta) = *factors.get(k - s).ok_or_else(bad)?;
                        let prev = *piece.get(k - 1).ok_or_else(bad)?;
                        let cur = piece.get_mut(k).ok_or_else(bad)?;
                        *cur = cur.blend(alpha, prev, beta);
                    }
                    if !end {
                        *next.get_mut(r - j).ok_or_else(bad)? = *piece.get(p).ok_or_else(bad)?;
                    }
                }
            }
            if !piece.iter().all(|h| h.is_sound()) {
                return invalid("the curve's points overflow, or its weights lose their precision");
            }
            if !end {
                for i in p - mult..=p {
                    *next.get_mut(i).ok_or_else(bad)? = *pw.get(b - p + i).ok_or_else(bad)?;
                }
                a = b;
                b += 1;
            }
            out.push(CurvePiece { span: [ua, ub], exact: std::mem::replace(&mut piece, next) });
        }
        Ok(out)
    }
}

/// One Bézier piece of a [`NurbsCurve`] over `span` of its parameter: `degree + 1` intervals that
/// hold its exact homogeneous control points.
#[derive(Clone, Debug, PartialEq)]
pub struct CurvePiece {
    span: [f64; 2],
    exact: Vec<HInterval>,
}

impl CurvePiece {
    /// The part of the curve's parameter the piece covers.
    pub fn span(&self) -> [f64; 2] {
        self.span
    }
    /// The homogeneous control points: each a point inside the interval that holds the exact one.
    pub fn ctrl(&self) -> impl Iterator<Item = HPoint> + '_ {
        self.exact.iter().map(|h| h.mid())
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::Lcg;
    use super::*;
    use crate::Vec3;

    /// A double-double number: `hi + lo` with `|lo|` at most half an ulp of `hi`, about 106 bits.
    /// The reference the curves are checked against, independent of this module's intervals.
    #[derive(Clone, Copy, Debug)]
    struct Dd(f64, f64);

    fn two_sum(a: f64, b: f64) -> (f64, f64) {
        let s = a + b;
        let v = s - a;
        (s, (a - (s - v)) + (b - v))
    }
    fn fast(a: f64, b: f64) -> Dd {
        let s = a + b;
        Dd(s, b - (s - a))
    }

    impl Dd {
        fn of(x: f64) -> Dd {
            Dd(x, 0.0)
        }
        fn add(self, o: Dd) -> Dd {
            let (s, e) = two_sum(self.0, o.0);
            let (t, f) = two_sum(self.1, o.1);
            let Dd(s, e) = fast(s, e + t);
            fast(s, e + f)
        }
        fn neg(self) -> Dd {
            Dd(-self.0, -self.1)
        }
        fn sub(self, o: Dd) -> Dd {
            self.add(o.neg())
        }
        fn mul(self, o: Dd) -> Dd {
            let p = self.0 * o.0;
            let e = self.0.mul_add(o.0, -p);
            fast(p, e + (self.0 * o.1 + self.1 * o.0))
        }
        fn div(self, o: Dd) -> Dd {
            let q1 = self.0 / o.0;
            let r = self.sub(o.mul(Dd::of(q1)));
            let q2 = r.0 / o.0;
            let r = r.sub(o.mul(Dd::of(q2)));
            fast(q1, q2).add(Dd::of(r.0 / o.0))
        }
        fn val(self) -> f64 {
            self.0 + self.1
        }
    }

    /// A curve as given: degree, knots and exact homogeneous control points in double-double.
    struct Def {
        p: usize,
        knots: Vec<f64>,
        h: Vec<[Dd; 4]>,
    }

    /// The homogeneous points over Cartesian `pts` and, when given, their weights.
    fn weighted(pts: &[Vec3], w: Option<&[f64]>) -> Vec<HPoint> {
        pts.iter().enumerate().map(|(i, &q)| HPoint::weighted(q, w.map_or(1.0, |w| w[i]))).collect()
    }

    /// A curve over Cartesian points and its definition.
    fn cartesian(p: usize, knots: &[f64], pts: &[Vec3], w: Option<&[f64]>) -> (NurbsCurve, Def) {
        let (curve, def) = homogeneous(p, knots, &weighted(pts, w));
        (curve.unwrap(), def)
    }

    /// A curve over homogeneous points (made or not) and its definition.
    fn homogeneous(p: usize, knots: &[f64], ctrl: &[HPoint]) -> (SplineResult<NurbsCurve>, Def) {
        let h = ctrl.iter().map(|q| [q.x, q.y, q.z, q.w].map(Dd::of)).collect();
        (NurbsCurve::from_homogeneous(p, knots.to_vec(), ctrl), Def { p, knots: knots.to_vec(), h })
    }

    /// A piece's point at `t ∈ [0, 1]`, from its `f64` control points.
    fn point(piece: &CurvePiece, t: f64) -> Vec3 {
        super::super::bezier_point(&piece.ctrl().collect::<Vec<_>>(), t).project().unwrap()
    }

    /// The exact curve's point at fraction `s` of `span`, by de Boor on its own knot interval in
    /// double-double (knots taken relative to the first, which double-double does exactly), both
    /// factors of each step from knot differences.
    fn truth(def: &Def, span: [f64; 2], s: f64) -> [Dd; 3] {
        let (p, kn) = (def.p, &def.knots);
        let n = def.h.len();
        let at = (p..n).rev().find(|&j| kn[j] <= span[0] && kn[j] < kn[j + 1]).unwrap();
        let k = |i: usize| Dd::of(kn[i]).sub(Dd::of(kn[0]));
        let (a, b) = (Dd::of(span[0]).sub(Dd::of(kn[0])), Dd::of(span[1]).sub(Dd::of(kn[0])));
        let u = a.add(Dd::of(s).mul(b.sub(a)));
        let mut d: Vec<[Dd; 4]> = (0..=p).map(|j| def.h[j + at - p]).collect();
        for r in 1..=p {
            for j in (r..=p).rev() {
                let i = j + at - p;
                let (lo, hi) = (k(i), k(i + p + 1 - r));
                let den = hi.sub(lo);
                let (f, g) = (hi.sub(u).div(den), u.sub(lo).div(den));
                d[j] = [0, 1, 2, 3].map(|c| d[j - 1][c].mul(f).add(d[j][c].mul(g)));
            }
        }
        let w = d[p][3];
        [0, 1, 2].map(|c| d[p][c].div(w))
    }

    /// Whether the exact curve's point at fraction `t` of the piece lies in the piece's enclosure
    /// there, evaluated in intervals.
    fn encloses(def: &Def, piece: &CurvePiece, t: f64) -> bool {
        let (tt, s) = (Interval::point(t), Interval::point(1.0).sub(Interval::point(t)));
        let mut pts = piece.exact.clone();
        while pts.len() > 1 {
            pts = pts.windows(2).map(|w| w[0].blend(s, w[1], tt)).collect();
        }
        let h = pts[0];
        let want = truth(def, piece.span(), t);
        [h.x, h.y, h.z].iter().zip(want).all(|(c, v)| {
            let held = c.div(h.w);
            held.lo.next_down() <= v.val() && v.val() <= held.hi.next_up()
        })
    }

    /// The pieces' spans are the domain's in order, each piece holds the curve, and its control
    /// points are close to the exact ones.
    fn assert_pieces_trace(curve: &NurbsCurve, def: &Def, want: usize) -> Vec<CurvePiece> {
        let (p, kn) = (def.p, &def.knots);
        let pieces = curve.pieces().unwrap();
        let mut spans: Vec<[f64; 2]> = (p..def.h.len()).map(|i| [kn[i], kn[i + 1]]).filter(|s| s[0] < s[1]).collect();
        spans.dedup();
        assert_eq!((pieces.len(), pieces.iter().map(|b| b.span()).collect::<Vec<_>>()), (want, spans));
        for b in &pieces {
            assert_eq!(b.ctrl().count(), p + 1);
            for t in [0.0, 0.17, 0.5, 0.83, 1.0] {
                assert!(encloses(def, b, t), "span {:?} at {t}", b.span());
                let got = point(b, t);
                let want = truth(def, b.span(), t).map(Dd::val);
                assert!(got.dist(Vec3::new(want[0], want[1], want[2])) < 1e-9, "span {:?} at {t}: {got:?}", b.span());
            }
        }
        pieces
    }

    fn plain(xy: &[(f64, f64)]) -> Vec<Vec3> {
        xy.iter().map(|&(x, y)| Vec3::new(x, y, 0.0)).collect()
    }

    #[test]
    fn a_curve_splits_into_the_bezier_pieces_of_its_own_knots() {
        let ctrl = plain(&[(0.0, 0.0), (10.0, 6.0), (18.0, -4.0), (30.0, 9.0), (36.0, 1.0), (48.0, 5.0), (52.0, -2.0)]);
        // Uneven gaps, as a DXF from another program has them; a double inner knot; an inner knot
        // repeated `degree` times, where the points are already Bézier points.
        for (knots, pieces) in [
            ([0.0, 0.0, 0.0, 0.0, 0.1, 0.15, 0.6, 1.0, 1.0, 1.0, 1.0], 4),
            ([0.0, 0.0, 0.0, 0.0, 0.3, 0.3, 0.7, 1.0, 1.0, 1.0, 1.0], 3),
            ([0.0, 0.0, 0.0, 0.0, 0.5, 0.5, 0.5, 1.0, 1.0, 1.0, 1.0], 2),
        ] {
            let (curve, def) = cartesian(3, &knots, &ctrl, None);
            assert_pieces_trace(&curve, &def, pieces);
        }
        // Rational, in space, at random.
        let mut rng = Lcg(9355);
        let rational: Vec<HPoint> =
            (0..8).map(|_| HPoint::weighted(Vec3::new(rng.unit(), rng.unit(), rng.unit()) * 10.0, 0.5 + rng.unit())).collect();
        let (curve, def) = homogeneous(3, &[0.0, 0.0, 0.0, 0.0, 0.3, 0.3, 0.5, 0.9, 1.0, 1.0, 1.0, 1.0], &rational);
        assert_pieces_trace(&curve.unwrap(), &def, 4);
        // A long one: 5000 points, uneven knots, one piece per span.
        let long = plain(&(0..5000).map(|i| (i as f64, (i as f64 / 7.0).sin())).collect::<Vec<_>>());
        let knots: Vec<f64> = (0..5004).map(|i| ((i.clamp(3, 5000) - 3) as f64).powf(1.3)).collect();
        assert_eq!(cartesian(3, &knots, &long, None).0.pieces().unwrap().len(), 4997);
    }

    #[test]
    fn weighted_quadratics_trace_circles_exactly() {
        let h = std::f64::consts::FRAC_1_SQRT_2;
        let c = [(1.0, 0.0), (1.0, 1.0), (0.0, 1.0), (-1.0, 1.0), (-1.0, 0.0), (-1.0, -1.0), (0.0, -1.0), (1.0, -1.0), (1.0, 0.0)];
        let pts = plain(&c.map(|(x, y)| (10.0 * x, 10.0 * y)));
        let knots = [0.0, 0.0, 0.0, 0.25, 0.25, 0.5, 0.5, 0.75, 0.75, 1.0, 1.0, 1.0];
        let (curve, def) = cartesian(2, &knots, &pts, Some(&[1.0, h, 1.0, h, 1.0, h, 1.0, h, 1.0]));
        for b in assert_pieces_trace(&curve, &def, 4) {
            for t in [0.0, 0.3, 0.6, 1.0] {
                assert!((point(&b, t).len() - 10.0).abs() < 1e-12);
            }
        }
    }

    #[test]
    fn an_unclamped_curve_is_cut_to_its_domain() {
        // A closed (periodic) cubic as DXF writes one: unclamped even knots; it doesn't start or
        // end on a control point, and ends where it starts.
        let ctrl = plain(&[(0.0, 0.0), (20.0, 0.0), (20.0, 20.0), (0.0, 20.0), (0.0, 0.0), (20.0, 0.0), (20.0, 20.0)]);
        let knots: Vec<f64> = (0..=10).map(f64::from).collect();
        let (curve, def) = cartesian(3, &knots, &ctrl, None);
        let pieces = assert_pieces_trace(&curve, &def, 4);
        let (first, last) = (point(&pieces[0], 0.0), point(&pieces[3], 1.0));
        assert!(first.dist(last) < 1e-9 && ctrl.iter().all(|q| q.dist(first) > 1.0), "{first:?} {last:?}");
        // Degree 9 (a G4 blend's) unclamped: eight insertions at each end.
        let mut rng = Lcg(9);
        let nine: Vec<Vec3> = (0..12).map(|_| Vec3::new(rng.unit(), rng.unit(), 0.0) * 10.0).collect();
        let knots: Vec<f64> = (0..22).map(f64::from).collect();
        let (curve, def) = cartesian(9, &knots, &nine, None);
        assert_pieces_trace(&curve, &def, 3);
    }

    #[test]
    fn a_curve_that_isnt_one_is_not_made() {
        let three = plain(&[(0.0, 0.0), (1.0, 1.0), (2.0, 0.0)]);
        let clamped = vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
        let made = |p: usize, knots: &[f64], pts: &[Vec3], w: Option<&[f64]>| NurbsCurve::from_homogeneous(p, knots.to_vec(), &weighted(pts, w));
        assert!(made(2, &clamped, &three, None).is_ok());
        for (p, knots) in [
            (2, &[0.0, 0.0, 0.0, 1.0, 1.0][..]),
            (2, &[0.0, 0.0, 1.0, 0.5, 1.0, 1.0]),
            (2, &[0.0, 0.0, 0.0, f64::NAN, 1.0, 1.0]),
            (2, &[0.0; 6]),
            (0, &clamped[..4]),
            (3, &[0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0]),
            (MAX_CURVE_DEGREE + 1, &[0.0; 21]),
        ] {
            assert!(made(p, knots, &three, None).is_err(), "{p} {knots:?}");
        }
        for w in [[1.0, 0.0, 1.0], [1.0, -1.0, 1.0], [1.0, f64::INFINITY, 1.0]] {
            assert!(made(2, &clamped, &three, Some(&w)).is_err(), "{w:?}");
        }
        assert!(made(2, &clamped, &plain(&[(f64::NAN, 0.0), (1.0, 1.0), (2.0, 0.0)]), None).is_err());
        assert!(made(2, &clamped, &plain(&[(2e12, 0.0), (1.0, 1.0), (2.0, 0.0)]), None).is_err());
        // A point times its weight past the largest number.
        assert!(made(2, &clamped, &plain(&[(1e12, 0.0), (1.0, 1.0), (2.0, 0.0)]), Some(&[1e300, 1.0, 1.0])).is_err());
        // Weights all scaled alike are the same curve.
        let small = made(2, &clamped, &three, Some(&[1e-200, 3e-200, 1e-200])).unwrap();
        let same = made(2, &clamped, &three, Some(&[1.0, 3.0, 1.0])).unwrap();
        let (a, b) = (&small.pieces().unwrap()[0], &same.pieces().unwrap()[0]);
        assert!(point(a, 0.3).dist(point(b, 0.3)) < 1e-15);
        // A line through three points breaks at an inner knot repeated twice (past degree 1).
        let four = plain(&[(0.0, 0.0), (1.0, 1.0), (2.0, 0.0), (3.0, 1.0)]);
        assert!(made(1, &[0.0, 0.0, 0.5, 1.0, 1.0], &four[..3], None).unwrap().pieces().is_ok());
        assert!(made(1, &[0.0, 0.0, 0.5, 0.5, 1.0, 1.0], &four, None).unwrap().pieces().is_err());
    }

    /// A knot inserted just short of the next, beside a weight of 10¹² and one of 1: `1 − α`,
    /// α = (1.5 − 2⁻⁵²)/1.5, rounds 2⁻⁵²/1.5 to 2⁻⁵³, 25 % off, which moved the piece's end by
    /// 8·10⁻⁴ mm. Both factors from knot differences, in intervals, keep it on the curve.
    #[test]
    fn a_knot_inserted_beside_a_heavy_weight_keeps_the_piece_on_the_curve() {
        let ub = 1.5 - 2f64.powi(-52);
        let pts = plain(&[(0.0, 0.0), (10.0, 20.0), (20.0, 0.0), (30.0, 10.0)]);
        let (curve, def) = cartesian(2, &[0.0, 0.0, 0.0, ub, 1.5, 1.5, 1.5], &pts, Some(&[1.0, 1e12, 1.0, 1.0]));
        let first = &curve.pieces().unwrap()[0];
        assert_eq!(first.span(), [0.0, ub]);
        for t in [0.5, 0.9, 1.0] {
            assert!(encloses(&def, first, t), "at {t}");
            let want = truth(&def, first.span(), t).map(Dd::val);
            let off = point(first, t).dist(Vec3::new(want[0], want[1], want[2]));
            assert!(off < 1e-9, "{off:e} off the curve at {t}");
        }
    }

    /// A random curve, valid or not: degree 1 to 9 (now and then 16), clamped or not, inner knots
    /// repeated up to and past the degree, knots offset far from zero with tiny spans, weights up
    /// to 10⁸ apart, sizes from a micrometre to 100 km.
    fn random_curve(rng: &mut Lcg) -> (usize, Vec<f64>, Vec<HPoint>) {
        let p = if rng.unit() < 0.05 { 16 } else { 1 + (rng.unit() * 9.0) as usize };
        let n = p + 1 + (rng.unit() * 12.0) as usize;
        let scale = 10f64.powf(-3.0 + rng.unit() * 11.0);
        let spread = 10f64.powf(rng.unit() * 8.0);
        let offset = if rng.unit() < 0.2 { 1e9 } else { 0.0 };
        let gap = if rng.unit() < 0.2 { 1e-6 } else { 1.0 };
        let mut knots = Vec::with_capacity(n + p + 1);
        let mut u = offset;
        let clamped = rng.unit() < 0.6;
        for i in 0..n + p + 1 {
            let inner = i > p && i < n;
            let repeat = inner && rng.unit() < 0.25;
            if !(clamped && (i <= p || i > n)) && !repeat && i > 0 {
                u += gap * (0.05 + rng.unit());
            }
            knots.push(u);
        }
        let ctrl = (0..n)
            .map(|_| {
                let w = spread.powf(rng.unit() - 0.5);
                HPoint::weighted(Vec3::new(rng.unit() - 0.5, rng.unit() - 0.5, if rng.unit() < 0.3 { rng.unit() } else { 0.0 }) * scale, w)
            })
            .collect();
        (p, knots, ctrl)
    }

    /// Thousands of random curves, valid and not: never a panic; a curve is either refused with a
    /// reason or split into pieces that hold it, checked in double-double. Enough of each happens
    /// for the test to mean something.
    #[test]
    fn random_curves_are_split_into_pieces_that_hold_them_or_refused() {
        let mut rng = Lcg(0x9355);
        let (mut made, mut refused) = (0, 0);
        for _ in 0..3000 {
            let (p, knots, ctrl) = random_curve(&mut rng);
            let (curve, def) = homogeneous(p, &knots, &ctrl);
            match curve.and_then(|c| c.pieces()) {
                Ok(pieces) => {
                    made += 1;
                    for piece in pieces.iter().take(2) {
                        for t in [0.0, 0.5, 1.0] {
                            assert!(encloses(&def, piece, t), "degree {p}, span {:?}, knots {knots:?}, at {t}", piece.span());
                        }
                    }
                }
                Err(e) => {
                    assert!(!e.to_string().is_empty());
                    refused += 1;
                }
            }
        }
        assert!(made > 1000 && refused > 100, "made {made}, refused {refused}");
    }
}
