//! Rational B-spline curves that are valid by construction, their exact Bézier pieces, and the
//! curves that follow a piece within a tolerance with a proven bound (#9355). 2D curves are curves
//! with z = 0.
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
//!
//! The bound of [`follow`]: a piece is `N(t) / W(t)`, homogeneous numerator over weight, and a
//! fit, written as a rational Bézier, `C(t) / V(t)`, so their difference at the same `t` is
//! `(C·W − N·V)(t) / (V·W)(t)`. In the Bernstein basis of degree `n + m` that numerator has vector
//! coefficients `dₖ` and the denominator positive ones `Wₖ`, both combinations with the same
//! non-negative basis, so the distance is at most `maxₖ ‖dₖ‖ / Wₖ`. Computed in intervals from the
//! enclosed piece and the fit, with `‖dₖ‖` taken as the sum of its components' sizes and `Wₖ` at
//! its smallest, the bound holds for the exact curve. A polynomial Bézier is the same point set
//! with weights `1, c, c², …` for any `c > 0` (and a conic with weights `1, ω·c, c²`), so `c` is
//! chosen to match the piece's own parametrisation; the bound then holds for the point sets.

use super::interval::{HInterval, Interval};
use super::{Blend, HPoint, SplineError, SplineResult, binomial, check_knots, clamp_row, invalid, subdivide};
use crate::Vec3;

/// Highest degree a curve may have: above the sketch's 9 ([`crate::MAX_BEZIER_DEGREE`]) for the
/// hidden-line views of imported curves. It bounds the work a hostile curve can ask for.
pub const MAX_CURVE_DEGREE: usize = 16;
/// Least `1 − rho` a conic [`Fit`] has: past it the sketch holds a conic's middle weight at
/// `rho / MIN_CONIC_RHO_GAP` rather than `rho / (1 − rho)`, a different curve.
pub const MIN_CONIC_RHO_GAP: f64 = 1e-9;
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

    pub fn degree(&self) -> usize {
        self.degree
    }
    pub fn knots(&self) -> &[f64] {
        &self.knots
    }
    /// The homogeneous control points, as the curve was made with them.
    pub fn ctrl(&self) -> impl Iterator<Item = HPoint> + '_ {
        self.exact.iter().map(|h| h.mid())
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

/// A curve that follows (part of) a curve piece, as a sketch holds curves.
#[derive(Clone, Debug, PartialEq)]
pub enum Fit {
    /// The polynomial Bézier curve over these control points; two make a line.
    Bezier(Vec<Vec3>),
    /// The conic from `a` to `b` with control point `apex` and shape `rho ∈ (0, 1)`: the rational
    /// quadratic with end weights 1 and middle weight `rho / (1 − rho)`.
    Conic { a: Vec3, apex: Vec3, b: Vec3, rho: f64 },
}

/// A value with a bound, proven when it was made, on how far it is from the part of a curve piece
/// it stands for: every point of either lies within `bound` of the other. Only this module makes
/// one.
#[derive(Clone, Debug, PartialEq)]
pub struct Certified<T> {
    value: T,
    bound: f64,
    along: [f64; 2],
}

impl<T> Certified<T> {
    pub fn value(&self) -> &T {
        &self.value
    }
    pub fn bound(&self) -> f64 {
        self.bound
    }
    /// The fractions of the piece's parameter between which the part it stands for lies.
    pub fn along(&self) -> [f64; 2] {
        self.along
    }
    pub fn into_value(self) -> T {
        self.value
    }
}

/// Curves that follow `piece` end to end, in order, each within `tolerance` of it with the bound
/// proven ([`Certified`]): the piece itself as a polynomial Bézier when it is one (up to its
/// parametrisation) and its degree is at most `max_degree`; a quadratic one as a conic; otherwise
/// cubics matching the ends of parts of it and their tangents, a part halved until its bound is
/// within the tolerance. Refused, with how close it came, when that would take more than
/// `max_fits` curves, or the intervals can't prove it (a tolerance below the piece's rounding).
pub fn follow(piece: &CurvePiece, tolerance: f64, max_degree: usize, max_fits: usize) -> SplineResult<Vec<Certified<Fit>>> {
    if !(tolerance.is_finite() && tolerance > 0.0) {
        return invalid("the tolerance must be a positive number");
    }
    // Every piece takes at least one curve; past this, curves made plus parts pending never
    // exceed the room (a split is checked before it adds one).
    if max_fits == 0 {
        return Err(SplineError::TooLarge("no room for a curve: a curve piece takes at least one".into()));
    }
    let mids: Vec<HPoint> = piece.ctrl().collect();
    let pts: Vec<Vec3> =
        mids.iter().map(|h| h.project()).collect::<Option<_>>().ok_or_else(|| SplineError::Invalid("a curve piece's points don't project".into()))?;
    let whole = |value: Fit, bound: f64| vec![Certified { value, bound, along: [0.0, 1.0] }];
    let (Some(first), Some(last)) = (mids.first(), mids.last()) else { return invalid("a curve piece has no points") };
    let n = mids.len() - 1;
    if (1..=max_degree).contains(&n) {
        // Weights 1, c, c², … with c from the end weights' ratio, so the parametrisations meet.
        let c = (last.w / first.w).powf(1.0 / n as f64);
        let c = Interval::point(if c.is_finite() && c > 0.0 { c } else { 1.0 });
        let weights: Vec<Interval> = std::iter::successors(Some(Interval::point(1.0)), |w| Some(w.mul(c))).take(n + 1).collect();
        if let Ok(bound) = error_bound(&piece.exact, &weighted(&pts, &weights))
            && bound <= tolerance
        {
            return Ok(whole(Fit::Bezier(pts), bound));
        }
    }
    if let ([a, apex, b], [h0, h1, h2]) = (&pts[..], &mids[..]) {
        // A conic whose apex lies on its chord is a line traced unevenly: no conic for a sketch.
        let side = (*apex - *a).cross(*b - *a).len();
        let w = h1.w / (h0.w * h2.w).sqrt();
        let rho = w / (1.0 + w);
        // Weights 1, ω·c, c² with c from the end weights' ratio, so the parametrisations meet.
        let c = (h2.w / h0.w).sqrt();
        // 1 − rho is exact for rho ≥ ½ (and far above the gap below it): the sketch's own test.
        if side > 1e-12 * a.dist(*apex) * a.dist(*b) && rho > 0.0 && 1.0 - rho >= MIN_CONIC_RHO_GAP && c.is_finite() && c > 0.0 {
            let rho_i = Interval::point(rho);
            let omega = rho_i.div(Interval::point(1.0).sub(rho_i));
            let c = Interval::point(c);
            if let Ok(bound) = error_bound(&piece.exact, &weighted(&pts, &[Interval::point(1.0), omega.mul(c), c.mul(c)]))
                && bound <= tolerance
            {
                return Ok(whole(Fit::Conic { a: *a, apex: *apex, b: *b, rho }, bound));
            }
        }
    }
    cubics(&piece.exact, tolerance, max_fits)
}

/// Points `pts` with the weights `w`, as homogeneous intervals.
fn weighted(pts: &[Vec3], w: &[Interval]) -> Vec<HInterval> {
    pts.iter()
        .zip(w)
        .map(|(p, &w)| HInterval { x: Interval::point(p.x).mul(w), y: Interval::point(p.y).mul(w), z: Interval::point(p.z).mul(w), w })
        .collect()
}

/// Cubic Hermite curves that follow the enclosed Bézier `exact`, halving a part until its bound is
/// within `tolerance` ([`follow`]).
fn cubics(exact: &[HInterval], tolerance: f64, max_fits: usize) -> SplineResult<Vec<Certified<Fit>>> {
    let half = Interval::point(0.5);
    let ones = [Interval::point(1.0); 4];
    let mut out = Vec::new();
    let mut parts = vec![(exact.to_vec(), [0.0, 1.0])];
    let mut mids = Vec::with_capacity(exact.len());
    while let Some((part, along)) = parts.pop() {
        mids.clear();
        mids.extend(part.iter().map(|h| h.mid()));
        let ctrl = hermite(&mids)?;
        let bound = error_bound(&part, &weighted(&ctrl, &ones))?;
        if bound <= tolerance {
            out.push(Certified { value: Fit::Bezier(ctrl.to_vec()), bound, along });
            continue;
        }
        if out.len() + parts.len() + 2 > max_fits {
            return Err(SplineError::TooLarge(format!(
                "more than {max_fits} curves to come within {tolerance} of a curve piece (one is {bound:.1e} off)"
            )));
        }
        let mid = f64::midpoint(along[0], along[1]);
        if !(along[0] < mid && mid < along[1]) {
            return Err(SplineError::Unsupported(format!("no cubic comes within {tolerance} of a curve piece: {bound:.1e} at the finest part")));
        }
        let (left, right) = subdivide(&part, half, half);
        if !left.iter().chain(&right).all(|h| h.is_sound()) {
            return invalid("the piece's points overflow, or its weights lose their precision");
        }
        parts.push((right, [mid, along[1]]));
        parts.push((left, [along[0], mid]));
    }
    Ok(out)
}

/// The cubic with the ends of the Bézier `q` and its tangents there (cubic Hermite): a rational
/// Bézier's derivative at its start is `n·(w₁/w₀)·(p₁ − p₀)`, and alike at its end. Any cubic would
/// do: the bound is computed for the one made.
fn hermite(q: &[HPoint]) -> SplineResult<[Vec3; 4]> {
    let n = q.len().saturating_sub(1);
    let at = |i: usize| q.get(i).and_then(|h| h.project().map(|p| (p, h.w)));
    let (Some((p0, w0)), Some((p1, w1)), Some((pm, wm)), Some((pn, wn))) = (at(0), at(1), at(n.saturating_sub(1)), at(n)) else {
        return invalid("a curve piece's points don't project");
    };
    let n = u32::try_from(n).map(f64::from).or_else(|_| invalid("a curve piece's degree is too high"))?;
    let (d0, d1) = ((p1 - p0) * (n * w1 / w0), (pn - pm) * (n * wm / wn));
    let ctrl = [p0, p0 + d0 / 3.0, pn - d1 / 3.0, pn];
    if !ctrl.iter().all(|p| p.is_finite()) {
        return invalid("a curve piece's tangents overflow");
    }
    Ok(ctrl)
}

/// A bound, proven in intervals, on the distance at each `t` from the rational Bézier `c` to the
/// one `q` stands for (module documentation).
fn error_bound(q: &[HInterval], c: &[HInterval]) -> SplineResult<f64> {
    let (Some(n), Some(m)) = (q.len().checked_sub(1), c.len().checked_sub(1)) else { return invalid("a Bézier has no points") };
    let mut worst = 0.0f64;
    for k in 0..=n + m {
        let (mut d, mut weight) = ([Interval::ZERO; 3], Interval::ZERO);
        // Binomials up to (32 k) are integers below 2⁵³, and so are the products here: exact.
        let all = Interval::point(binomial(n + m, k));
        for j in k.saturating_sub(n)..=k.min(m) {
            // A term left out would make the bound a guess, so a missing point is an error.
            let (Some(h), Some(cj)) = (q.get(k - j), c.get(j)) else { return invalid("a Bézier's points are missing") };
            let coef = Interval::point(binomial(m, j) * binomial(n, k - j)).div(all);
            for (dc, (hc, cc)) in d.iter_mut().zip([(h.x, cj.x), (h.y, cj.y), (h.z, cj.z)]) {
                *dc = dc.add(coef.mul(cc.mul(h.w).sub(hc.mul(cj.w))));
            }
            weight = weight.add(coef.mul(h.w.mul(cj.w)));
        }
        if !(weight.lo > 0.0) {
            return Err(SplineError::Unsupported("a curve piece's weights have lost their precision".into()));
        }
        // ‖dₖ‖ ≤ |dx| + |dy| + |dz|, rounded up.
        let length = d.iter().fold(Interval::ZERO, |acc, c| acc.add(Interval::point(c.mag())));
        worst = worst.max(Interval::point(length.hi).div(Interval::point(weight.lo)).hi);
    }
    if worst.is_finite() { Ok(worst) } else { invalid("a curve piece's bound overflows") }
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
    fn truth(def: &Def, span: [f64; 2], s: Dd) -> [Dd; 3] {
        let (p, kn) = (def.p, &def.knots);
        let n = def.h.len();
        let at = (p..n).rev().find(|&j| kn[j] <= span[0] && kn[j] < kn[j + 1]).unwrap();
        let k = |i: usize| Dd::of(kn[i]).sub(Dd::of(kn[0]));
        let (a, b) = (Dd::of(span[0]).sub(Dd::of(kn[0])), Dd::of(span[1]).sub(Dd::of(kn[0])));
        let u = a.add(s.mul(b.sub(a)));
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
        let want = truth(def, piece.span(), Dd::of(t));
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
                let want = truth(def, b.span(), Dd::of(t)).map(Dd::val);
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
            let want = truth(&def, first.span(), Dd::of(t)).map(Dd::val);
            let off = point(first, t).dist(Vec3::new(want[0], want[1], want[2]));
            assert!(off < 1e-9, "{off:e} off the curve at {t}");
        }
    }

    /// A random curve, valid or not: degree 1 to 9 (now and then 16), clamped or not, inner knots
    /// repeated up to and past the degree, knots offset far from zero with tiny spans, weights up
    /// to 10⁸ apart, sizes from a micrometre to 100 km.
    fn random_curve(rng: &mut Lcg) -> (usize, Vec<f64>, Vec<HPoint>, f64) {
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
        (p, knots, ctrl, scale)
    }

    /// Thousands of random curves, valid and not: never a panic; a curve is either refused with a
    /// reason or split into pieces that hold it, checked in double-double; a piece's fits either
    /// come within the tolerance, the bound checked both ways in double-double, or are refused
    /// with the reason. Enough of each happens for the test to mean something.
    #[test]
    fn random_curves_are_split_and_followed_with_proof_or_refused() {
        let mut rng = Lcg(0x9355);
        let (mut made, mut refused, mut certified, mut declined) = (0, 0, 0, 0);
        for i in 0..3000 {
            let (p, knots, ctrl, scale) = random_curve(&mut rng);
            let (curve, def) = homogeneous(p, &knots, &ctrl);
            let pieces = match curve.and_then(|c| c.pieces()) {
                Ok(pieces) => pieces,
                Err(e) => {
                    assert!(!e.to_string().is_empty());
                    refused += 1;
                    continue;
                }
            };
            made += 1;
            for piece in pieces.iter().take(2) {
                for t in [0.0, 0.5, 1.0] {
                    assert!(encloses(&def, piece, t), "degree {p}, span {:?}, knots {knots:?}, at {t}", piece.span());
                }
            }
            // A tolerance a millionth of the size, then one near rounding with little room; the
            // bound checked both ways on a tenth of the curves (it is slow in double-double).
            let Some(piece) = pieces.first() else { continue };
            for (tolerance, room) in [(1e-6 * scale, 4096), (1e-13 * scale, 64)] {
                match follow(piece, tolerance, crate::MAX_BEZIER_DEGREE, room) {
                    Ok(fits) => {
                        assert_follows(&def, piece, &fits, tolerance, if i % 10 == 0 { 3 } else { 0 });
                        certified += 1;
                    }
                    Err(e) => {
                        assert!(!e.to_string().is_empty());
                        declined += 1;
                    }
                }
            }
        }
        assert!(
            made > 1000 && refused > 100 && certified > 1000 && declined > 100,
            "made {made}, refused {refused}, certified {certified}, declined {declined}"
        );
    }

    /// A fit's point at `s` of its own parameter, in double-double.
    fn fit_point(fit: &Fit, s: Dd) -> [Dd; 3] {
        let one = Dd::of(1.0);
        let (pts, w): (Vec<Vec3>, Vec<Dd>) = match fit {
            Fit::Bezier(p) => (p.clone(), vec![one; p.len()]),
            Fit::Conic { a, apex, b, rho } => (vec![*a, *apex, *b], vec![one, Dd::of(*rho).div(one.sub(Dd::of(*rho))), one]),
        };
        let r = one.sub(s);
        let mut h: Vec<[Dd; 4]> = pts.iter().zip(&w).map(|(p, &w)| [Dd::of(p.x).mul(w), Dd::of(p.y).mul(w), Dd::of(p.z).mul(w), w]).collect();
        while h.len() > 1 {
            h = h.windows(2).map(|q| [0, 1, 2, 3].map(|c| q[0][c].mul(r).add(q[1][c].mul(s)))).collect();
        }
        [0, 1, 2].map(|c| h[0][c].div(h[0][3]))
    }

    fn dist(a: [Dd; 3], b: [Dd; 3]) -> f64 {
        (0..3).fold(Dd::of(0.0), |acc, c| acc.add(a[c].sub(b[c]).mul(a[c].sub(b[c])))).val().sqrt()
    }

    /// The distance from `x` to the curve `f` over `[0, 1]`: samples spread along the curve, not
    /// its parameter (a rational curve can run most of its length in a sliver of it), and closer
    /// together the nearer they come to `x` (a curve can loop tighter than the spacing), each one
    /// closer than its neighbours refined by golden-section search in double-double. Never less
    /// than the true distance (each is to a point on the curve), so a check against it can't pass
    /// wrongly.
    fn closest(f: &dyn Fn(Dd) -> [Dd; 3], x: [Dd; 3]) -> f64 {
        let mut s: Vec<Dd> = (0..=64).map(|i| Dd::of(f64::from(i) / 64.0)).collect();
        let mut p: Vec<[Dd; 3]> = s.iter().map(|&t| f(t)).collect();
        let length: f64 = p.windows(2).map(|w| dist(w[0], w[1])).sum();
        for _ in 0..60 {
            let (mut ns, mut np) = (vec![s[0]], vec![p[0]]);
            for i in 1..s.len() {
                let near = 0.5 * dist(p[i - 1], x).min(dist(p[i], x));
                if dist(p[i - 1], p[i]) > near.min(length / 256.0) && s[i].sub(s[i - 1]).val() > 1e-28 && ns.len() < 20_000 {
                    let mid = s[i - 1].add(s[i].sub(s[i - 1]).mul(Dd::of(0.5)));
                    ns.push(mid);
                    np.push(f(mid));
                }
                ns.push(s[i]);
                np.push(p[i]);
            }
            if ns.len() == s.len() {
                break;
            }
            (s, p) = (ns, np);
        }
        let d: Vec<f64> = p.iter().map(|q| dist(*q, x)).collect();
        let last = d.len() - 1;
        let mut found = d.iter().copied().fold(f64::INFINITY, f64::min);
        let g = Dd::of(0.381_966_011_250_105_1);
        for i in (0..=last).filter(|&i| d[i] <= d[i.saturating_sub(1)] && d[i] <= d[(i + 1).min(last)]) {
            let (mut lo, mut hi) = (s[i.saturating_sub(1)], s[(i + 1).min(last)]);
            for _ in 0..100 {
                let (m1, m2) = (lo.add(hi.sub(lo).mul(g)), hi.sub(hi.sub(lo).mul(g)));
                let (d1, d2) = (dist(f(m1), x), dist(f(m2), x));
                found = found.min(d1).min(d2);
                if d1 < d2 { hi = m2 } else { lo = m1 }
            }
        }
        found
    }

    /// The fits end to end over the piece, each bound within the tolerance; and on `check` fits
    /// (the first, last and middle ones), every point of each within its bound of the part of the
    /// piece it follows, both ways, at seven fractions, in double-double.
    fn assert_follows(def: &Def, piece: &CurvePiece, fits: &[Certified<Fit>], tolerance: f64, check: usize) {
        assert_eq!(fits.first().map(|c| c.along()[0]), Some(0.0));
        assert_eq!(fits.last().map(|c| c.along()[1]), Some(1.0));
        assert!(fits.windows(2).all(|w| w[0].along()[1] == w[1].along()[0]));
        assert!(fits.iter().all(|c| c.bound() <= tolerance));
        let mut picks = vec![0, fits.len() / 2, fits.len() - 1];
        picks.dedup();
        for c in picks.into_iter().take(check).map(|i| &fits[i]) {
            let [a, b] = c.along();
            let part = |s: Dd| truth(def, piece.span(), Dd::of(a).add(s.mul(Dd::of(b).sub(Dd::of(a)))));
            let fit = |s: Dd| fit_point(c.value(), s);
            // The rounding of the check's own square root.
            let allowed = c.bound() * (1.0 + 1e-9);
            for i in 0..=6 {
                let s = Dd::of(f64::from(i) / 6.0);
                let (to_fit, to_part) = (closest(&fit, part(s)), closest(&part, fit(s)));
                assert!(to_fit <= allowed && to_part <= allowed, "{to_fit:e}, {to_part:e} past the bound {:e} of {:?}", c.bound(), c.value());
            }
        }
    }

    /// A piece that is a polynomial Bézier, up to how its parameter runs, is followed by itself:
    /// a cubic, the same with weights 1, 2, 4, 8 (the same curve, traced unevenly), a line with
    /// weights 1 and 7. Above the degree allowed it is followed by cubics.
    #[test]
    fn a_piece_that_is_a_polynomial_bezier_is_followed_by_itself() {
        let pts = plain(&[(0.0, 0.0), (5.0, 30.0), (40.0, 30.0), (50.0, 0.0)]);
        let cubic = [0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0];
        for (knots, pts, w) in
            [(&cubic[..], &pts[..], None), (&cubic, &pts, Some(&[1.0, 2.0, 4.0, 8.0][..])), (&[0.0, 0.0, 1.0, 1.0], &pts[..2], Some(&[1.0, 7.0]))]
        {
            let (curve, def) = cartesian(knots.len() - pts.len() - 1, knots, pts, w);
            let piece = &curve.pieces().unwrap()[0];
            let fits = follow(piece, 1e-9, 9, 100).unwrap();
            assert!(matches!(&fits[..], [c] if *c.value() == Fit::Bezier(pts.to_vec()) && c.bound() < 1e-12), "{w:?}: {fits:?}");
            assert_follows(&def, piece, &fits, 1e-9, 1);
        }
        let five = plain(&[(0.0, 0.0), (5.0, 30.0), (20.0, -10.0), (30.0, 40.0), (40.0, 30.0), (50.0, 0.0)]);
        let (curve, def) = cartesian(5, &[0.0; 6].into_iter().chain([1.0; 6]).collect::<Vec<_>>(), &five, None);
        let piece = &curve.pieces().unwrap()[0];
        let fits = follow(piece, 1e-6, 3, 1000).unwrap();
        assert!(fits.len() > 1 && fits.iter().all(|c| matches!(c.value(), Fit::Bezier(p) if p.len() == 4)), "{fits:?}");
        assert_follows(&def, piece, &fits, 1e-6, 3);
    }

    /// Weighted quadratics (how CAD programs write arcs) are followed by conics on their circle; one
    /// sharper than a sketch conic holds, or whose points lie on a line, is not a conic.
    #[test]
    fn weighted_quadratics_are_followed_by_conics() {
        let h = std::f64::consts::FRAC_1_SQRT_2;
        let pts = plain(&[(10.0, 0.0), (10.0, 10.0), (0.0, 10.0)]);
        let knots = [0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
        // Written with end weights other than 1, as some programs do.
        for w in [[1.0, h, 1.0], [2.0, 3.0 * h, 4.5]] {
            let (curve, def) = cartesian(2, &knots, &pts, Some(&w));
            let piece = &curve.pieces().unwrap()[0];
            let fits = follow(piece, 1e-9, 9, 100).unwrap();
            let [c] = &fits[..] else { panic!("{fits:?}") };
            assert!(matches!(c.value(), Fit::Conic { .. }) && c.bound() < 1e-12, "{w:?}: {c:?}");
            assert_follows(&def, piece, &fits, 1e-9, 1);
        }
        // Middle weight 2·10⁹: 1 − rho is below the sketch's least, so no conic, though its bound
        // would pass.
        let (curve, def) = cartesian(2, &knots, &pts, Some(&[1.0, 2e9, 1.0]));
        let piece = &curve.pieces().unwrap()[0];
        let fits = follow(piece, 1e-3, 9, 1000).unwrap();
        assert!(fits.iter().all(|c| !matches!(c.value(), Fit::Conic { .. })), "{fits:?}");
        assert_follows(&def, piece, &fits, 1e-3, 3);
        let (curve, def) = cartesian(2, &knots, &plain(&[(0.0, 0.0), (30.0, 0.0), (10.0, 0.0)]), Some(&[1.0, 3.0, 1.0]));
        let piece = &curve.pieces().unwrap()[0];
        let fits = follow(piece, 1e-6, 9, 1000).unwrap();
        assert!(fits.iter().all(|c| !matches!(c.value(), Fit::Conic { .. })), "{fits:?}");
        assert_follows(&def, piece, &fits, 1e-6, 3);
    }

    #[test]
    fn a_weighted_cubic_is_followed_within_its_tolerance_with_proof() {
        let pts = plain(&[(0.0, 0.0), (5.0, 30.0), (40.0, 30.0), (50.0, 0.0)]);
        let knots = [0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0];
        let (curve, def) = cartesian(3, &knots, &pts, Some(&[1.0, 3.0, 0.5, 1.0]));
        let piece = &curve.pieces().unwrap()[0];
        for tolerance in [1e-1, 1e-3, 1e-6] {
            let fits = follow(piece, tolerance, 9, 10_000).unwrap();
            assert_follows(&def, piece, &fits, tolerance, 3);
            // Each halving gains about 16x, so a tolerance a thousand times tighter takes about
            // six times the cubics, not a thousand.
            assert!(fits.len() < 200, "{tolerance}: {} cubics", fits.len());
        }
        // Room for one curve is enough for a piece one curve follows; none is refused.
        let (plain, _) = cartesian(3, &knots, &pts, None);
        let piece = &plain.pieces().unwrap()[0];
        assert_eq!(follow(piece, 1e-9, 9, 1).unwrap().len(), 1);
        assert!(matches!(follow(piece, 1e-9, 9, 0), Err(SplineError::TooLarge(_))));
    }

    #[test]
    fn a_follow_past_its_limits_says_so() {
        let pts = plain(&[(0.0, 0.0), (0.0, 1e5), (1e5, 3e4), (2e4, 0.0)]);
        let (curve, def) = cartesian(3, &[0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0], &pts, Some(&[1.0, 1e4, 1e-4, 1.0]));
        let piece = &curve.pieces().unwrap()[0];
        // Too few curves allowed; a tolerance below rounding; no tolerance at all.
        assert!(matches!(follow(piece, 1e-3, 9, 8), Err(SplineError::TooLarge(_))));
        assert!(follow(piece, 1e-30, 9, usize::MAX).is_err());
        for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert!(follow(piece, bad, 9, 100).is_err(), "{bad}");
        }
        // Given room, the same 100 m curve with weights 10⁸ apart does come within 0.001 mm, proven.
        let fits = follow(piece, 1e-3, 9, 100_000).unwrap();
        assert_follows(&def, piece, &fits, 1e-3, 3);
    }

    /// A curve about 10⁻²⁰⁰ across: its bounds stay above zero and hold, where lengths taken as the
    /// root of the squares, which underflow there, made every bound 0.
    #[test]
    fn a_tiny_curve_is_bounded_and_held() {
        let s = 2f64.powi(-664);
        let pts: Vec<Vec3> = plain(&[(0.0, 0.0), (5.0, 30.0), (40.0, 30.0), (50.0, 0.0)]).into_iter().map(|p| p * s).collect();
        let (curve, def) = cartesian(3, &[0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0], &pts, Some(&[1.0, 3.0, 0.5, 1.0]));
        let piece = &curve.pieces().unwrap()[0];
        assert!(follow(piece, 1e-250, 9, 1000).is_err());
        let fits = follow(piece, 1e-3 * s, 9, 10_000).unwrap();
        assert!(fits.iter().all(|c| c.bound() > 0.0));
        assert_follows(&def, piece, &fits, 1e-3 * s, 3);
    }

    /// A piece over two adjacent numbers of the parameter (near 10⁹): its fits, tied to fractions
    /// of the piece rather than to its parameter, still follow it.
    #[test]
    fn a_piece_with_no_room_in_its_parameter_is_followed() {
        let (a, b) = (1e9, f64::from_bits(1e9f64.to_bits() + 1));
        let pts = plain(&[(0.0, 0.0), (5.0, 30.0), (40.0, 30.0), (50.0, 0.0)]);
        let (curve, def) = cartesian(3, &[a, a, a, a, b, b, b, b], &pts, Some(&[1.0, 3.0, 0.5, 1.0]));
        let piece = &curve.pieces().unwrap()[0];
        assert_eq!(piece.span(), [a, b]);
        let fits = follow(piece, 1e-3, 9, 1000).unwrap();
        assert_follows(&def, piece, &fits, 1e-3, 3);
    }
}
