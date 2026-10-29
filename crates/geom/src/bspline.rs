//! B-spline machinery shared by NURBS and T-spline surfaces: homogeneous points, the
//! univariate basis (Cox–de Boor on half-open intervals, with derivatives), Boehm knot
//! insertion, knot insertion into a single basis function, Bézier extraction, and a tensor
//! NURBS surface.
//!
//! References: Piegl & Tiller, *The NURBS Book* (2nd ed.) — §2.2 (basis definition, eq. 2.5),
//! §2.3 (derivatives, eq. 2.9), A2.1 (span search), §5.2 (knot insertion, eq. 5.10 / A5.1),
//! §1.4 (de Casteljau); Sederberg, Cardon, Finnigan, North, Zheng, Lyche 2004, "T-spline
//! simplification and local refinement", §3 (knot insertion into one blending function).
//!
//! Input is hostile: every constructor checks finiteness and sizes and returns
//! [`SplineError`] instead of panicking; no function here indexes with an unchecked number.

use std::ops::{Add, AddAssign, Mul, Sub};

use crate::Vec3;

mod parameter_line;

/// Highest polynomial degree accepted in either direction.
pub const MAX_SPLINE_DEGREE: usize = 7;
/// Most knots accepted in one knot vector.
pub const MAX_KNOTS: usize = 1 << 14;
/// Most control points accepted in one tensor surface.
pub const MAX_SURFACE_POINTS: usize = 1 << 20;

/// Why a spline could not be built or used.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum SplineError {
    #[error("invalid spline: {0}")]
    Invalid(String),
    #[error("not supported yet: {0}")]
    Unsupported(String),
    #[error("spline too large: {0}")]
    TooLarge(String),
}

pub type SplineResult<T> = Result<T, SplineError>;

pub(crate) fn invalid<T>(msg: impl Into<String>) -> SplineResult<T> {
    Err(SplineError::Invalid(msg.into()))
}

/// A homogeneous control point `(x·w, y·w, z·w, w)`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HPoint {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub w: f64,
}

impl HPoint {
    pub const ZERO: HPoint = HPoint { x: 0.0, y: 0.0, z: 0.0, w: 0.0 };

    /// The point `p` with weight `w`.
    pub fn weighted(p: Vec3, w: f64) -> HPoint {
        HPoint { x: p.x * w, y: p.y * w, z: p.z * w, w }
    }
    /// The Cartesian point, if the weight is usable.
    pub fn project(self) -> Option<Vec3> {
        let p = Vec3::new(self.x / self.w, self.y / self.w, self.z / self.w);
        (self.w.abs() > f64::MIN_POSITIVE && p.is_finite()).then_some(p)
    }
    /// The weighted coordinates `(x·w, y·w, z·w)`.
    pub fn xyz(self) -> Vec3 {
        Vec3::new(self.x, self.y, self.z)
    }
    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite() && self.w.is_finite()
    }
}

impl Add for HPoint {
    type Output = HPoint;
    fn add(self, o: HPoint) -> HPoint {
        HPoint { x: self.x + o.x, y: self.y + o.y, z: self.z + o.z, w: self.w + o.w }
    }
}
impl Sub for HPoint {
    type Output = HPoint;
    fn sub(self, o: HPoint) -> HPoint {
        HPoint { x: self.x - o.x, y: self.y - o.y, z: self.z - o.z, w: self.w - o.w }
    }
}
impl Mul<f64> for HPoint {
    type Output = HPoint;
    fn mul(self, k: f64) -> HPoint {
        HPoint { x: self.x * k, y: self.y * k, z: self.z * k, w: self.w * k }
    }
}
impl AddAssign for HPoint {
    fn add_assign(&mut self, o: HPoint) {
        *self = *self + o;
    }
}

/// A parameter direction: along s (u, the first) or along t (v, the second).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Dir {
    S,
    T,
}

impl Dir {
    /// `s` for [`Dir::S`], `t` for [`Dir::T`].
    pub fn pick<X>(self, s: X, t: X) -> X {
        match self {
            Dir::S => s,
            Dir::T => t,
        }
    }
}

/// Homogeneous control points from finite points and positive weights, one each.
pub fn weighted_points(points: &[Vec3], weights: &[f64]) -> SplineResult<Vec<HPoint>> {
    if points.len() != weights.len() {
        return invalid("one weight per control point");
    }
    if points.iter().any(|p| !p.is_finite()) {
        return invalid("a control point is not finite");
    }
    if weights.iter().any(|w| !(w.is_finite() && *w > 0.0)) {
        return invalid("weights must be positive numbers");
    }
    Ok(points.iter().zip(weights).map(|(p, w)| HPoint::weighted(*p, *w)).collect())
}

/// Which one-sided limit a basis evaluation takes at a knot. Basis functions live on half-open
/// spans `[uᵢ, uᵢ₊₁)` ([`Side::Right`]); at the right end of a domain the left limit on
/// `(uᵢ, uᵢ₊₁]` ([`Side::Left`]) keeps the closing span.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Right,
    Left,
}

/// Check a knot vector: finite, non-decreasing, not too long.
pub fn check_knots(knots: &[f64]) -> SplineResult<()> {
    if knots.len() > MAX_KNOTS {
        return Err(SplineError::TooLarge(format!("{} knots (at most {MAX_KNOTS})", knots.len())));
    }
    if knots.iter().any(|k| !k.is_finite()) {
        return invalid("a knot is not a finite number");
    }
    if knots.windows(2).any(|w| matches!(w, [a, b] if b < a)) {
        return invalid("knots must not decrease");
    }
    Ok(())
}

/// One B-spline basis function of degree `p = knots.len() − 2` given by its local knot vector
/// `[ξ₀ … ξₚ₊₁]`, and its derivatives up to order `n`: element `k` is the `k`-th derivative at
/// `u`. Cox–de Boor recursion (NURBS Book eq. 2.5) with the 0/0 = 0 convention, derivatives by
/// eq. 2.9 applied repeatedly (`dᵏNᵢ,ₚ = p·(dᵏ⁻¹Nᵢ,ₚ₋₁/(uᵢ₊ₚ−uᵢ) − dᵏ⁻¹Nᵢ₊₁,ₚ₋₁/(uᵢ₊ₚ₊₁−uᵢ₊₁))`).
/// A knot vector shorter than two knots gives zeros.
pub fn local_basis(knots: &[f64], u: f64, side: Side, n: usize) -> Vec<f64> {
    let mut out = vec![0.0; n + 1];
    let ders = local_basis_array(knots, u, side, n);
    for (o, d) in out.iter_mut().zip(ders) {
        *o = d;
    }
    out
}

/// [`local_basis`] into a fixed array (no allocation): element `k` is the `k`-th derivative,
/// for `k ≤ min(n, MAX_SPLINE_DEGREE)`, the rest zero.
pub(crate) fn local_basis_array(knots: &[f64], u: f64, side: Side, n: usize) -> [f64; MAX_SPLINE_DEGREE + 1] {
    const W: usize = MAX_SPLINE_DEGREE + 1;
    let mut out = [0.0; W];
    let Some(p) = knots.len().checked_sub(2) else { return out };
    if p > MAX_SPLINE_DEGREE || !u.is_finite() {
        return out;
    }
    let kn = |i: usize| knots.get(i).copied().unwrap_or(0.0);
    let ratio = |num: f64, den: f64| if den > 0.0 { num / den } else { 0.0 };
    // table[k][j]: k-th derivative of N_{j,q} for the current degree q (j = 0..=p−q), updated
    // in place: k descending (level k reads level k−1 of the previous degree), j ascending
    // (N_{j,q} reads N_{j+1,q−1}, not yet overwritten). Derivatives above p are zero.
    let levels = n.min(p);
    let mut table = [[0.0f64; W]; W];
    for (j, slot) in table[0].iter_mut().enumerate().take(p + 1) {
        let (a, b) = (kn(j), kn(j + 1));
        let inside = match side {
            Side::Right => a <= u && u < b,
            Side::Left => a < u && u <= b,
        };
        *slot = if inside { 1.0 } else { 0.0 };
    }
    for q in 1..=p {
        for k in (0..=levels).rev() {
            for j in 0..=p - q {
                let (d1, d2) = (kn(j + q) - kn(j), kn(j + q + 1) - kn(j + 1));
                let at = |k: usize, j: usize| table.get(k).and_then(|r| r.get(j)).copied().unwrap_or(0.0);
                let v = if k == 0 {
                    ratio(u - kn(j), d1) * at(0, j) + ratio(kn(j + q + 1) - u, d2) * at(0, j + 1)
                } else {
                    q as f64 * (ratio(at(k - 1, j), d1) - ratio(at(k - 1, j + 1), d2))
                };
                if let Some(slot) = table.get_mut(k).and_then(|r| r.get_mut(j)) {
                    *slot = v;
                }
            }
        }
    }
    for (k, o) in out.iter_mut().enumerate().take(levels + 1) {
        *o = table.get(k).map_or(0.0, |r| r[0]);
    }
    out
}

/// The coefficients `(c₁, c₂)` of knot insertion into one basis function: inserting `k`
/// (with `ξ₀ ≤ k ≤ ξₚ₊₁`) into `N[ξ₀…ξₚ₊₁]` gives `c₁·N[first p+2 merged knots] +
/// c₂·N[last p+2 merged knots]` (Sederberg et al. 2004, §3; the single-function case of the
/// NURBS Book eq. 5.10). `None` when `k` is outside the support or the knots are too few.
pub fn split_basis(knots: &[f64], k: f64) -> Option<(f64, f64)> {
    let p = knots.len().checked_sub(2)?;
    let (x0, x1, xp, xl) = (*knots.first()?, *knots.get(1)?, *knots.get(p)?, *knots.get(p + 1)?);
    if !(k >= x0 && k <= xl) {
        return None;
    }
    let c1 = if k >= xp { 1.0 } else { (k - x0) / (xp - x0) };
    let c2 = if k <= x1 { 1.0 } else { (xl - k) / (xl - x1) };
    Some((c1, c2))
}

/// The two knot vectors of a split basis function (see [`split_basis`]): `k` merged into
/// `knots` in order, then the first and the last `knots.len()` entries.
pub fn split_knots<T: Copy + PartialOrd>(knots: &[T], k: T) -> [Vec<T>; 2] {
    let at = knots.iter().position(|&x| x > k).unwrap_or(knots.len());
    let mut merged = knots.to_vec();
    merged.insert(at, k);
    let n = knots.len();
    [merged.get(..n).map(<[T]>::to_vec).unwrap_or_default(), merged.get(1..).map(<[T]>::to_vec).unwrap_or_default()]
}

/// The Bernstein coefficients of one basis function `N[knots]` on `[a, b]`: `N(u) =
/// Σ cᵢ Bᵢ,ₚ((u−a)/(b−a))` there. `[a, b]` must not contain a knot in its interior (the
/// function is one polynomial on it). Found by inserting `a` and `b` into the function until
/// every surviving piece is a Bernstein polynomial `N[a…a, b…b]` (Sederberg et al. 2004, §3;
/// NURBS Book §5.4, decomposition into Bézier segments).
pub fn basis_bezier(knots: &[f64], a: f64, b: f64) -> SplineResult<Vec<f64>> {
    let Some(p) = knots.len().checked_sub(2) else { return invalid("a basis function needs at least two knots") };
    if p > MAX_SPLINE_DEGREE {
        return Err(SplineError::Unsupported(format!("degree {p} (at most {MAX_SPLINE_DEGREE})")));
    }
    check_knots(knots)?;
    if !(a.is_finite() && b.is_finite() && a < b) {
        return invalid("a Bézier interval must be finite and non-empty");
    }
    if knots.iter().any(|&x| x > a && x < b) {
        return invalid("a knot lies inside the Bézier interval");
    }
    let mut out = vec![0.0; p + 1];
    let mut work = vec![(knots.to_vec(), 1.0)];
    let mut steps = 0usize;
    while let Some((kn, c)) = work.pop() {
        steps += 1;
        if steps > 1 << 12 {
            return Err(SplineError::TooLarge("Bézier extraction did not converge".into()));
        }
        let (Some(&lo), Some(&hi)) = (kn.first(), kn.last()) else { continue };
        if hi <= a || lo >= b || c == 0.0 {
            continue;
        }
        let insert = if kn.iter().any(|&x| x < a) {
            Some(a)
        } else if kn.iter().any(|&x| x > b) {
            Some(b)
        } else {
            None
        };
        match insert {
            Some(k) => {
                let (c1, c2) = split_basis(&kn, k).ok_or_else(|| SplineError::Invalid("knot insertion outside the support".into()))?;
                let [first, second] = split_knots(&kn, k);
                work.push((first, c * c1));
                work.push((second, c * c2));
            }
            None => {
                // Knots a×(p+1−i), b×(i+1): the Bernstein polynomial Bᵢ,ₚ.
                let nb = kn.iter().filter(|&&x| x == b).count();
                if let Some(slot) = nb.checked_sub(1).and_then(|i| out.get_mut(i)) {
                    *slot += c;
                }
            }
        }
    }
    Ok(out)
}

/// The span index `k` with `knots[k] ≤ u < knots[k+1]` among the valid spans `p ≤ k < n`
/// (`n` control points), the last span closed at the domain end (NURBS Book A2.1).
pub fn find_span(knots: &[f64], p: usize, n: usize, u: f64) -> Option<usize> {
    if n <= p || knots.len() != n + p + 1 || !u.is_finite() {
        return None;
    }
    let (lo, hi) = (*knots.get(p)?, *knots.get(n)?);
    if u < lo || u > hi {
        return None;
    }
    // The last non-empty span in [p, n−1] that starts at or before u: by bisection (the knots
    // are sorted), and at the domain end the last non-empty span.
    let mut k = if u >= hi {
        n - 1
    } else {
        let below = knots.get(p..n)?.partition_point(|&x| x <= u);
        p + below.checked_sub(1)?
    };
    while k > p && !(knots.get(k)? < knots.get(k + 1)?) {
        k -= 1;
    }
    Some(k)
}

/// Insert the knot `u` once into a B-spline with degree `p`, knots and homogeneous control
/// points (Boehm; NURBS Book eq. 5.10 / A5.1). `u` must lie in the domain `[knots[p],
/// knots[n]]`; the curve is unchanged.
pub fn insert_knot(p: usize, knots: &[f64], ctrl: &[HPoint], u: f64) -> SplineResult<(Vec<f64>, Vec<HPoint>)> {
    let n = ctrl.len();
    if n <= p || knots.len() != n + p + 1 {
        return invalid("knot and control point counts do not match the degree");
    }
    if knots.len() >= MAX_KNOTS {
        return Err(SplineError::TooLarge("too many knots".into()));
    }
    check_knots(knots)?;
    let k = find_span(knots, p, n, u).ok_or_else(|| SplineError::Invalid(format!("knot {u} is outside the domain")))?;
    let kn = |i: usize| knots.get(i).copied().unwrap_or(0.0);
    let pt = |i: usize| ctrl.get(i).copied().unwrap_or(HPoint::ZERO);
    let mut q = Vec::with_capacity(n + 1);
    for i in 0..=n {
        q.push(if i + p <= k {
            pt(i)
        } else if i > k {
            pt(i - 1)
        } else {
            let den = kn(i + p) - kn(i);
            let alpha = if den > 0.0 { (u - kn(i)) / den } else { 1.0 };
            pt(i) * alpha + pt(i - 1) * (1.0 - alpha)
        });
    }
    let mut nk = knots.to_vec();
    nk.insert(k + 1, u);
    Ok((nk, q))
}

/// The binomial coefficient `(n k)` (zero for `k > n`).
pub(crate) fn binomial(n: usize, k: usize) -> f64 {
    if k > n {
        return 0.0;
    }
    (0..k).fold(1.0, |c, m| c * (n - m) as f64 / (m + 1) as f64)
}

/// Bernstein polynomial `Bᵢ,ₚ(x)`.
pub fn bernstein(p: usize, i: usize, x: f64) -> f64 {
    if i > p {
        return 0.0;
    }
    binomial(p, i) * x.powi(i as i32) * (1.0 - x).powi((p - i) as i32)
}

/// One row of a B-spline (degree `p`, knots, homogeneous control points) clamped on its domain
/// `[knots[p], knots[n]]`: each domain end inserted until it repeats `p` times (§5.2), then the
/// basis functions that vanish on the domain dropped (a basis function's last span does not
/// depend on its first knot, so that knot moves to the domain end). The curve on the domain is
/// unchanged.
pub(crate) fn clamp_row(p: usize, knots: &[f64], row: &[HPoint]) -> SplineResult<(Vec<f64>, Vec<HPoint>)> {
    let n = row.len();
    let (Some(&a), Some(&b)) = (knots.get(p), knots.get(n)) else { return invalid("too few knots") };
    let (mut knots, mut row) = (knots.to_vec(), row.to_vec());
    for (at_end, x) in [(false, a), (true, b)] {
        let mult = |k: &[f64]| k.iter().filter(|&&y| y == x).count();
        let mut guard = 0;
        while mult(&knots) < p {
            guard += 1;
            if guard > MAX_SPLINE_DEGREE + 1 {
                return invalid("clamping did not converge");
            }
            (knots, row) = insert_knot(p, &knots, &row, x)?;
        }
        let n = row.len();
        let (first, last) = if at_end {
            // Functions i with i ≥ (first index of x) start at or past the end.
            (0, knots.iter().position(|&y| y == x).unwrap_or(n).min(n))
        } else {
            // Functions i with i + p + 1 ≤ (last index of x) end at or before the start.
            (knots.iter().rposition(|&y| y == x).unwrap_or(p).saturating_sub(p), n)
        };
        if last <= first + p {
            return invalid("too few control points remain after clamping");
        }
        let mut nk: Vec<f64> = knots.get(first..last + p + 1).map(<[f64]>::to_vec).unwrap_or_default();
        if at_end {
            nk.last_mut().into_iter().for_each(|y| *y = x);
        } else {
            nk.first_mut().into_iter().for_each(|y| *y = x);
        }
        row = row.get(first..last).map(<[HPoint]>::to_vec).unwrap_or_default();
        knots = nk;
    }
    Ok((knots, row))
}

/// A Bézier curve's point at `x ∈ [0, 1]` (de Casteljau, NURBS Book §1.4).
pub fn bezier_point(ctrl: &[HPoint], x: f64) -> HPoint {
    subdivide(ctrl, x).0.last().copied().unwrap_or(HPoint::ZERO)
}

/// The control points of the piece of a Bézier curve over `[a, b] ⊂ [0, 1]` (two de Casteljau
/// subdivisions).
pub fn bezier_segment(ctrl: &[HPoint], a: f64, b: f64) -> SplineResult<Vec<HPoint>> {
    if !(a.is_finite() && b.is_finite() && 0.0 <= a && a < b && b <= 1.0) {
        return invalid("a Bézier segment must satisfy 0 ≤ a < b ≤ 1");
    }
    // Left part [0, b], then its right part from a/b.
    let left = subdivide(ctrl, b).0;
    Ok(subdivide(&left, a / b).1)
}

/// Split a Bézier curve at `x` (de Casteljau): control points of `[0, x]` and `[x, 1]`.
fn subdivide(ctrl: &[HPoint], x: f64) -> (Vec<HPoint>, Vec<HPoint>) {
    let mut left = Vec::with_capacity(ctrl.len());
    let mut right = Vec::with_capacity(ctrl.len());
    let mut pts = ctrl.to_vec();
    while let (Some(&f), Some(&l)) = (pts.first(), pts.last()) {
        left.push(f);
        right.push(l);
        pts = pts.windows(2).map(|w| if let [a, b] = w { *a * (1.0 - x) + *b * x } else { HPoint::ZERO }).collect();
    }
    right.reverse();
    (left, right)
}

/// A rational tensor Bézier patch over the parameter rectangle `s × t`: control point `(i, j)`
/// (i along s, j along t) at `ctrl[j·(degree[0]+1) + i]`.
#[derive(Clone, Debug, PartialEq)]
pub struct BezierPatch {
    pub s: [f64; 2],
    pub t: [f64; 2],
    pub degree: [usize; 2],
    pub ctrl: Vec<HPoint>,
}

impl BezierPatch {
    /// Control point `(i, j)`.
    pub fn point(&self, i: usize, j: usize) -> HPoint {
        self.ctrl.get(j * (self.degree[0] + 1) + i).copied().unwrap_or(HPoint::ZERO)
    }
    /// Homogeneous point at global parameters `(s, t)`.
    pub fn eval_h(&self, s: f64, t: f64) -> HPoint {
        let x = (s - self.s[0]) / (self.s[1] - self.s[0]);
        let y = (t - self.t[0]) / (self.t[1] - self.t[0]);
        let mut acc = HPoint::ZERO;
        for j in 0..=self.degree[1] {
            let bj = bernstein(self.degree[1], j, y);
            for i in 0..=self.degree[0] {
                acc += self.point(i, j) * (bernstein(self.degree[0], i, x) * bj);
            }
        }
        acc
    }
    /// Point at global parameters `(s, t)`.
    pub fn eval(&self, s: f64, t: f64) -> Option<Vec3> {
        self.eval_h(s, t).project()
    }
    /// The control points of one side as a Bézier curve: `0` t = t₀ (s rising), `1` s = s₁
    /// (t rising), `2` t = t₁ (s rising), `3` s = s₀ (t rising).
    pub fn side(&self, side: usize) -> Vec<HPoint> {
        let [ps, pt] = self.degree;
        match side {
            0 => (0..=ps).map(|i| self.point(i, 0)).collect(),
            1 => (0..=pt).map(|j| self.point(ps, j)).collect(),
            2 => (0..=ps).map(|i| self.point(i, pt)).collect(),
            _ => (0..=pt).map(|j| self.point(0, j)).collect(),
        }
    }
}

/// A rational tensor-product B-spline (NURBS) surface. Control point `(i, j)` (i along u, j
/// along v) is `ctrl[j·count_u + i]`; the domain is `[knots_u[p], knots_u[n]] × …`.
#[derive(Clone, Debug, PartialEq)]
pub struct NurbsSurface {
    degree: [usize; 2],
    knots: [Vec<f64>; 2],
    count: [usize; 2],
    ctrl: Vec<HPoint>,
}

impl NurbsSurface {
    /// A surface from Cartesian points and positive weights (one per point, u fastest).
    pub fn new(degree: [usize; 2], knots: [Vec<f64>; 2], points: &[Vec3], weights: &[f64]) -> SplineResult<NurbsSurface> {
        Self::from_homogeneous(degree, knots, weighted_points(points, weights)?)
    }

    /// A surface from homogeneous control points (u fastest).
    pub fn from_homogeneous(degree: [usize; 2], knots: [Vec<f64>; 2], ctrl: Vec<HPoint>) -> SplineResult<NurbsSurface> {
        let mut count = [0; 2];
        for d in 0..2 {
            let (p, kn) = (degree[d], &knots[d]);
            if p == 0 || p > MAX_SPLINE_DEGREE {
                return Err(SplineError::Unsupported(format!("degree {p} (1 to {MAX_SPLINE_DEGREE})")));
            }
            check_knots(kn)?;
            let n = kn.len().checked_sub(p + 1).filter(|&n| n > p).ok_or_else(|| SplineError::Invalid("too few knots for the degree".into()))?;
            if !(kn.get(p) < kn.get(n)) {
                return invalid("the surface's domain is empty");
            }
            count[d] = n;
        }
        if count[0].checked_mul(count[1]).is_none_or(|c| c > MAX_SURFACE_POINTS) {
            return Err(SplineError::TooLarge(format!("more than {MAX_SURFACE_POINTS} control points")));
        }
        if ctrl.len() != count[0] * count[1] {
            return invalid(format!("{} control points, expected {}", ctrl.len(), count[0] * count[1]));
        }
        if ctrl.iter().any(|h| !h.is_finite() || !(h.w > 0.0)) {
            return invalid("control points must be finite with positive weights");
        }
        Ok(NurbsSurface { degree, knots, count, ctrl })
    }

    pub fn degree(&self) -> [usize; 2] {
        self.degree
    }
    pub fn knots(&self) -> &[Vec<f64>; 2] {
        &self.knots
    }
    /// Control point counts along u and v.
    pub fn count(&self) -> [usize; 2] {
        self.count
    }
    pub fn homogeneous(&self) -> &[HPoint] {
        &self.ctrl
    }
    /// The control net.
    pub fn net(&self) -> crate::Grid<HPoint> {
        crate::Grid::checked(self.count, self.ctrl.clone())
    }
    /// Control point `(i, j)`.
    pub fn point(&self, i: usize, j: usize) -> HPoint {
        self.ctrl.get(j * self.count[0] + i).copied().unwrap_or(HPoint::ZERO)
    }

    /// The parameter domain `[[u₀, u₁], [v₀, v₁]]`.
    pub fn domain(&self) -> [[f64; 2]; 2] {
        let d = |k: usize| {
            let (p, n, kn) = (self.degree[k], self.count[k], &self.knots[k]);
            [kn.get(p).copied().unwrap_or(0.0), kn.get(n).copied().unwrap_or(0.0)]
        };
        [d(0), d(1)]
    }

    /// The point at `(u, v)`, which must lie in the domain.
    pub fn eval(&self, u: f64, v: f64) -> SplineResult<Vec3> {
        let dom = self.domain();
        let mut spans = [0; 2];
        let mut basis: [Vec<f64>; 2] = [Vec::new(), Vec::new()];
        for (d, x) in [u, v].into_iter().enumerate() {
            let (p, n, kn) = (self.degree[d], self.count[d], &self.knots[d]);
            let span = find_span(kn, p, n, x).ok_or_else(|| SplineError::Invalid(format!("({u}, {v}) is outside the domain")))?;
            let side = if x >= dom[d][1] { Side::Left } else { Side::Right };
            spans[d] = span;
            basis[d] = (span - p..=span).map(|i| kn.get(i..i + p + 2).map_or(0.0, |k| local_basis(k, x, side, 0)[0])).collect();
        }
        let mut acc = HPoint::ZERO;
        for (b, nj) in basis[1].iter().enumerate() {
            for (a, ni) in basis[0].iter().enumerate() {
                acc += self.point(spans[0] - self.degree[0] + a, spans[1] - self.degree[1] + b) * (ni * nj);
            }
        }
        acc.project().ok_or_else(|| SplineError::Invalid("the surface's weights vanish there".into()))
    }

    /// An exact rational isoparametric curve segment. `dir` is its varying
    /// parameter; the other parameter is fixed. The returned ends are clamped.
    pub fn iso_curve_segment(&self, dir: Dir, fixed: f64, range: [f64; 2]) -> SplineResult<(Vec<f64>, Vec<HPoint>)> {
        let along = dir.pick(0, 1);
        let across = 1 - along;
        let p = self.degree[across];
        let knots = &self.knots[across];
        let span =
            find_span(knots, p, self.count[across], fixed).ok_or_else(|| SplineError::Invalid("isocurve parameter is outside the surface".into()))?;
        let side = if fixed >= self.domain()[across][1] { Side::Left } else { Side::Right };
        let basis: Vec<_> = (span - p..=span).map(|i| knots.get(i..i + p + 2).map_or(0.0, |k| local_basis(k, fixed, side, 0)[0])).collect();
        let mut ctrl = Vec::with_capacity(self.count[along]);
        for i in 0..self.count[along] {
            let mut point = HPoint::ZERO;
            for (j, weight) in basis.iter().enumerate() {
                let k = span - p + j;
                point += if along == 0 { self.point(i, k) } else { self.point(k, i) } * *weight;
            }
            ctrl.push(point);
        }
        restrict_row(self.degree[along], &self.knots[along], &ctrl, range)
    }

    /// The same surface with the knot `x` inserted once in direction `dir` ([`Dir::S`] = u).
    pub fn insert_knot(&self, dir: Dir, x: f64) -> SplineResult<NurbsSurface> {
        let d = dir.pick(0, 1);
        let (p, kn) = (self.degree[d], &self.knots[d]);
        let [nu, nv] = self.count;
        let (lines, len) = if d == 0 { (nv, nu) } else { (nu, nv) };
        let at = |line: usize, k: usize| if d == 0 { self.point(k, line) } else { self.point(line, k) };
        let mut new_knots = Vec::new();
        let mut rows: Vec<Vec<HPoint>> = Vec::with_capacity(lines);
        for line in 0..lines {
            let row: Vec<HPoint> = (0..len).map(|k| at(line, k)).collect();
            let (nk, nr) = insert_knot(p, kn, &row, x)?;
            new_knots = nk;
            rows.push(nr);
        }
        let mut knots = self.knots.clone();
        knots[d] = new_knots;
        let new_len = len + 1;
        let mut ctrl = Vec::with_capacity(lines * new_len);
        if d == 0 {
            for row in &rows {
                ctrl.extend_from_slice(row);
            }
        } else {
            for j in 0..new_len {
                for row in &rows {
                    ctrl.push(row.get(j).copied().unwrap_or(HPoint::ZERO));
                }
            }
        }
        NurbsSurface::from_homogeneous(self.degree, knots, ctrl)
    }

    /// The same surface on the same domain with clamped knot vectors (`p+1` equal knots at
    /// each end, so the boundary curves are given by the boundary rows of control points): each
    /// row of control points clamped in turn ([`clamp_row`]).
    pub fn clamped(&self) -> SplineResult<NurbsSurface> {
        let mut s = self.clone();
        for d in 0..2 {
            let p = s.degree[d];
            let [nu, nv] = s.count;
            let (lines, len) = if d == 0 { (nv, nu) } else { (nu, nv) };
            let mut knots = Vec::new();
            let mut rows = Vec::with_capacity(lines);
            for line in 0..lines {
                let row: Vec<HPoint> = (0..len).map(|k| if d == 0 { s.point(k, line) } else { s.point(line, k) }).collect();
                let (k, r) = clamp_row(p, &s.knots[d], &row)?;
                knots = k;
                rows.push(r);
            }
            let new_len = rows.first().map_or(0, Vec::len);
            let mut ctrl = Vec::with_capacity(lines * new_len);
            if d == 0 {
                rows.iter().for_each(|r| ctrl.extend_from_slice(r));
            } else {
                for j in 0..new_len {
                    ctrl.extend(rows.iter().map(|r| r.get(j).copied().unwrap_or(HPoint::ZERO)));
                }
            }
            let mut all = s.knots.clone();
            all[d] = knots;
            s = NurbsSurface::from_homogeneous(s.degree, all, ctrl)?;
        }
        Ok(s)
    }
}

fn restrict_row(p: usize, knots: &[f64], ctrl: &[HPoint], [lo, hi]: [f64; 2]) -> SplineResult<(Vec<f64>, Vec<HPoint>)> {
    let (Some(&start), Some(&end)) = (knots.get(p), knots.get(ctrl.len())) else { return invalid("invalid curve domain") };
    if !lo.is_finite() || !hi.is_finite() || lo < start || hi > end || lo >= hi {
        return invalid("curve restriction must be a nonempty interval inside its domain");
    }
    let (mut knots, mut ctrl) = (knots.to_vec(), ctrl.to_vec());
    for x in [lo, hi] {
        for _ in 0..=p {
            if knots.iter().filter(|&&k| k == x).count() > p {
                break;
            }
            (knots, ctrl) = insert_knot(p, &knots, &ctrl, x)?;
        }
    }
    let first = knots.iter().position(|&k| k == lo).ok_or_else(|| SplineError::Invalid("missing curve restriction start".into()))?;
    let last = knots.iter().rposition(|&k| k == hi).ok_or_else(|| SplineError::Invalid("missing curve restriction end".into()))? + 1;
    let count = (last - first).checked_sub(p + 1).ok_or_else(|| SplineError::Invalid("invalid restricted knot count".into()))?;
    let row = ctrl.get(first..first + count).ok_or_else(|| SplineError::Invalid("invalid restricted control count".into()))?.to_vec();
    Ok((knots[first..last].to_vec(), row))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A small deterministic generator for test data.
    pub(crate) struct Lcg(pub(crate) u64);
    impl Lcg {
        pub(crate) fn unit(&mut self) -> f64 {
            self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (self.0 >> 11) as f64 / (1u64 << 53) as f64
        }
    }

    const KNOTS: [f64; 12] = [0.0, 0.0, 0.0, 0.0, 0.3, 0.3, 0.5, 0.9, 1.0, 1.0, 1.0, 1.0];

    fn samples() -> Vec<f64> {
        (0..=200).map(|k| k as f64 / 200.0).collect()
    }

    #[test]
    fn basis_partition_of_unity_non_negative_compact_support() {
        let p = 3;
        let n = KNOTS.len() - p - 1;
        for u in samples() {
            let side = if u >= 1.0 { Side::Left } else { Side::Right };
            let mut sum = 0.0;
            for i in 0..n {
                let local = &KNOTS[i..i + p + 2];
                let b = local_basis(local, u, side, 0)[0];
                assert!(b >= -1e-15, "N{i}({u}) = {b}");
                if u < local[0] || u > local[p + 1] {
                    assert_eq!(b, 0.0, "N{i} is not zero outside its support at {u}");
                }
                sum += b;
            }
            assert!((sum - 1.0).abs() < 1e-14, "Σ N({u}) = {sum}");
        }
    }

    #[test]
    fn basis_derivatives_match_finite_differences() {
        let p = 3;
        let h = 1e-6;
        let mut worst = 0.0f64;
        for i in 0..KNOTS.len() - p - 1 {
            let local = &KNOTS[i..i + p + 2];
            for u in samples() {
                if KNOTS.iter().any(|k| (k - u).abs() < 2.0 * h) {
                    continue;
                }
                let d = local_basis(local, u, Side::Right, 2);
                let f = |x: f64| local_basis(local, x, Side::Right, 0)[0];
                let fd1 = (f(u + h) - f(u - h)) / (2.0 * h);
                let df = |x: f64| local_basis(local, x, Side::Right, 1)[1];
                let fd2 = (df(u + h) - df(u - h)) / (2.0 * h);
                worst = worst.max((d[1] - fd1).abs()).max((d[2] - fd2).abs() / 100.0);
            }
        }
        assert!(worst < 1e-5, "derivative error {worst}");
    }

    #[test]
    fn span_search_finds_the_last_non_empty_span_at_or_before_u() {
        let p = 3;
        let knots = [0.0, 0.0, 0.0, 0.0, 0.3, 0.3, 0.5, 0.9, 0.9, 0.9, 1.0, 1.0, 1.0, 1.0];
        let n = knots.len() - p - 1;
        for k in 0..=1000 {
            let u = k as f64 / 1000.0;
            // The plain search: the last span [knots[i], knots[i+1]) with knots[i] ≤ u.
            let want = (p..n).filter(|&i| knots[i] <= u && knots[i] < knots[i + 1]).max().unwrap();
            assert_eq!(find_span(&knots, p, n, u), Some(want), "u = {u}");
        }
        assert_eq!(find_span(&knots, p, n, 1.5), None);
        assert_eq!(find_span(&knots, p, n, f64::NAN), None);
    }

    #[test]
    fn single_function_knot_insertion_is_exact() {
        let mut rng = Lcg(7);
        for _ in 0..200 {
            let mut kn: Vec<f64> = (0..5).map(|_| (rng.unit() * 8.0).round() / 8.0).collect();
            kn.sort_by(f64::total_cmp);
            if kn[0] == kn[4] {
                continue;
            }
            let k = kn[0] + (kn[4] - kn[0]) * (rng.unit() * 8.0).round() / 8.0;
            let (c1, c2) = split_basis(&kn, k).unwrap();
            let mut merged = kn.clone();
            let at = merged.iter().position(|&x| x > k).unwrap_or(5);
            merged.insert(at, k);
            for u in samples() {
                let u = kn[0] - 0.1 + u * (kn[4] - kn[0] + 0.2);
                let f = |k: &[f64]| local_basis(k, u, Side::Right, 0)[0];
                let lhs = f(&kn);
                let rhs = c1 * f(&merged[..5]) + c2 * f(&merged[1..]);
                assert!((lhs - rhs).abs() < 1e-14, "{kn:?} insert {k} at {u}: {lhs} vs {rhs}");
            }
        }
    }

    #[test]
    fn basis_bezier_coefficients_reproduce_the_function() {
        let p = 3;
        let spans: Vec<(f64, f64)> = KNOTS.windows(2).filter(|w| w[0] < w[1]).map(|w| (w[0], w[1])).collect();
        for i in 0..KNOTS.len() - p - 1 {
            let local = &KNOTS[i..i + p + 2];
            for &(a, b) in &spans {
                let c = basis_bezier(local, a, b).unwrap();
                for k in 0..=20 {
                    let x = k as f64 / 20.0;
                    let u = a + (b - a) * x;
                    let side = if x >= 1.0 { Side::Left } else { Side::Right };
                    let want = local_basis(local, u, side, 0)[0];
                    let got: f64 = c.iter().enumerate().map(|(j, cj)| cj * bernstein(p, j, x)).sum();
                    assert!((want - got).abs() < 1e-14, "N{i} on [{a}, {b}] at {u}: {want} vs {got}");
                }
            }
        }
        assert!(basis_bezier(&KNOTS[..5], 0.0, 0.5).is_err(), "a knot inside the interval is refused");
        assert!(basis_bezier(&KNOTS[..5], f64::NAN, 0.5).is_err());
    }

    fn curve(p: usize, knots: &[f64], ctrl: &[HPoint], u: f64) -> Vec3 {
        let n = ctrl.len();
        let side = if u >= knots[n] { Side::Left } else { Side::Right };
        let mut acc = HPoint::ZERO;
        for (i, c) in ctrl.iter().enumerate() {
            acc += *c * local_basis(&knots[i..i + p + 2], u, side, 0)[0];
        }
        acc.project().unwrap()
    }

    #[test]
    fn arbitrary_parameter_lines_retain_rational_surface_positions_in_both_directions() {
        let surface = random_surface(117, KNOTS.to_vec(), KNOTS.to_vec(), [3, 3]);
        for (a, b) in [
            ([0.0, 0.0], [1.0, 1.0]),
            ([0.0, 4e-14], [1.0, 1.0 - 4e-14]),
            ([0.0, 0.0], [1.0, 1.0 - 4e-13]),
            ([0.11, 0.17], [0.93, 0.81]),
            ([0.9, 0.13], [0.07, 0.87]),
            ([0.7, 1.0], [0.7, 0.0]),
            ([1.0, 0.2], [0.0, 0.2]),
        ] {
            for (a, b) in [(a, b), (b, a)] {
                let (knots, ctrl) = surface.parameter_line(a, b).unwrap();
                let degree = knots.len() - ctrl.len() - 1;
                for i in 0..=100 {
                    let t = i as f64 / 100.0;
                    let want = surface.eval(a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t).unwrap();
                    assert!(curve(degree, &knots, &ctrl, t).dist(want) < 1e-9, "{a:?}->{b:?}, {t}");
                }
            }
        }
        for (a, b) in [([0.5, 0.5], [0.5, 0.5]), ([f64::NAN, 0.0], [1.0, 1.0]), ([-0.1, 0.0], [1.0, 1.0])] {
            assert!(surface.parameter_line(a, b).is_err());
        }
    }
    #[test]
    fn trimmed_isocurves_retain_rational_surface_positions() {
        let surface = random_surface(91, KNOTS.to_vec(), KNOTS.to_vec(), [3, 3]);
        for dir in [Dir::S, Dir::T] {
            for fixed in [0.0, 0.17, 0.5, 1.0] {
                for range in [[0.0, 1.0], [0.125, 0.7], [0.3, 0.9]] {
                    let (knots, ctrl) = surface.iso_curve_segment(dir, fixed, range).unwrap();
                    assert!(knots[..4].iter().all(|&k| k == range[0]));
                    assert!(knots[knots.len() - 4..].iter().all(|&k| k == range[1]));
                    for i in 0..=20 {
                        let t = if i == 20 { range[1] } else { range[0] + (range[1] - range[0]) * i as f64 / 20.0 };
                        let (u, v) = if dir == Dir::S { (t, fixed) } else { (fixed, t) };
                        assert!(curve(3, &knots, &ctrl, t).dist(surface.eval(u, v).unwrap()) < 1e-11);
                    }
                }
            }
        }
        for range in [[0.8, 0.2], [-0.1, 1.0], [0.0, f64::NAN]] {
            assert!(surface.iso_curve_segment(Dir::S, 0.5, range).is_err());
        }
        assert!(surface.iso_curve_segment(Dir::S, f64::INFINITY, [0.0, 1.0]).is_err());
    }

    #[test]
    fn boehm_insertion_keeps_the_curve() {
        let p = 3;
        let mut rng = Lcg(3);
        let ctrl: Vec<HPoint> = (0..8).map(|_| HPoint::weighted(Vec3::new(rng.unit(), rng.unit(), rng.unit()) * 10.0, 0.5 + rng.unit())).collect();
        let (mut kn, mut pts) = (KNOTS.to_vec(), ctrl.clone());
        for u in [0.0, 0.1, 0.3, 0.3, 0.77, 1.0, 1.0] {
            (kn, pts) = insert_knot(p, &kn, &pts, u).unwrap();
        }
        let mut worst = 0.0f64;
        for u in samples() {
            worst = worst.max(curve(p, &KNOTS, &ctrl, u).dist(curve(p, &kn, &pts, u)));
        }
        assert!(worst < 1e-13, "deviation {worst}");
        assert!(insert_knot(p, &KNOTS, &ctrl, 1.5).is_err());
        assert!(insert_knot(p, &KNOTS, &ctrl, f64::NAN).is_err());
    }

    pub(crate) fn random_surface(seed: u64, ku: Vec<f64>, kv: Vec<f64>, degree: [usize; 2]) -> NurbsSurface {
        let mut rng = Lcg(seed);
        let n = (ku.len() - degree[0] - 1) * (kv.len() - degree[1] - 1);
        let pts: Vec<Vec3> = (0..n).map(|_| Vec3::new(rng.unit(), rng.unit(), rng.unit()) * 10.0).collect();
        let w: Vec<f64> = (0..n).map(|_| 0.5 + rng.unit()).collect();
        NurbsSurface::new(degree, [ku, kv], &pts, &w).unwrap()
    }

    fn max_deviation(a: &NurbsSurface, b: &NurbsSurface) -> f64 {
        let [du, dv] = a.domain();
        let mut worst = 0.0f64;
        for i in 0..=30 {
            for j in 0..=30 {
                let u = du[0] + (du[1] - du[0]) * i as f64 / 30.0;
                let v = dv[0] + (dv[1] - dv[0]) * j as f64 / 30.0;
                worst = worst.max(a.eval(u, v).unwrap().dist(b.eval(u, v).unwrap()));
            }
        }
        worst
    }

    #[test]
    fn surface_knot_insertion_and_clamping_keep_the_surface() {
        let uniform: Vec<f64> = (0..10).map(|k| k as f64).collect();
        let s = random_surface(11, uniform.clone(), (0..9).map(|k| k as f64 * 0.5).collect(), [3, 3]);
        let r = s.insert_knot(Dir::S, 4.5).unwrap().insert_knot(Dir::T, 2.25).unwrap();
        assert!(max_deviation(&s, &r) < 1e-12);
        let c = s.clamped().unwrap();
        assert_eq!(c.domain(), s.domain());
        assert_eq!(&c.knots()[0][..4], &[3.0; 4]);
        assert_eq!(&c.knots()[1][c.knots()[1].len() - 4..], &[2.5; 4]);
        let dev = max_deviation(&s, &c);
        assert!(dev < 1e-12, "clamping moved the surface by {dev}");
        // A clamped surface interpolates its corner control points.
        let corner = c.eval(3.0, 1.5).unwrap();
        assert!(corner.dist(c.point(0, 0).project().unwrap()) < 1e-12);
    }

    #[test]
    fn bezier_segments_lie_on_the_curve() {
        let mut rng = Lcg(5);
        let ctrl: Vec<HPoint> = (0..4).map(|_| HPoint::weighted(Vec3::new(rng.unit(), rng.unit(), 0.0), 0.5 + rng.unit())).collect();
        let seg = bezier_segment(&ctrl, 0.2, 0.7).unwrap();
        for k in 0..=10 {
            let x = k as f64 / 10.0;
            let want = bezier_point(&ctrl, 0.2 + 0.5 * x).project().unwrap();
            assert!(bezier_point(&seg, x).project().unwrap().dist(want) < 1e-14);
        }
        assert!(bezier_segment(&ctrl, 0.7, 0.2).is_err());
    }

    #[test]
    fn hostile_spline_input_is_refused() {
        let pts = [Vec3::ZERO; 16];
        let w = [1.0; 16];
        let k = vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0];
        assert!(NurbsSurface::new([3, 3], [k.clone(), k.clone()], &pts, &w).is_ok());
        assert!(NurbsSurface::new([3, 3], [vec![0.0, f64::NAN, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0], k.clone()], &pts, &w).is_err());
        assert!(NurbsSurface::new([3, 3], [k.clone(), k.clone()], &pts, &[0.0; 16]).is_err());
        assert!(NurbsSurface::new([3, 3], [k.clone(), k.clone()], &pts[..15], &w[..15]).is_err());
        assert!(NurbsSurface::new([0, 3], [k.clone(), k.clone()], &pts, &w).is_err());
        assert!(NurbsSurface::new([9, 3], [k.clone(), k.clone()], &pts, &w).is_err());
        assert!(NurbsSurface::new([3, 3], [vec![1.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0], k.clone()], &pts, &w).is_err());
        assert!(check_knots(&vec![0.0; MAX_KNOTS + 1]).is_err());
        let s = NurbsSurface::new([3, 3], [k.clone(), k], &pts, &w).unwrap();
        assert!(s.eval(2.0, 0.5).is_err());
        assert!(s.eval(f64::NAN, 0.5).is_err());
        assert_eq!(local_basis(&[], 0.5, Side::Right, 1), vec![0.0, 0.0]);
    }
}

/// A non-rational B-spline curve whose control points have `N` coordinates: a point in space
/// together with other values that vary along it (an intersection curve with its parameters on
/// both surfaces).
#[derive(Clone, Debug, PartialEq)]
pub struct CurveN<const N: usize> {
    pub degree: usize,
    pub knots: Vec<f64>,
    pub ctrl: Vec<[f64; N]>,
}

impl<const N: usize> CurveN<N> {
    /// The curve of degree `degree` through `points` at the increasing parameters `params`
    /// (global interpolation, The NURBS Book A9.1: clamped, interior knots by averaging the
    /// parameters).
    pub fn interpolate(params: &[f64], points: &[[f64; N]], degree: usize) -> SplineResult<CurveN<N>> {
        let m = points.len();
        if params.len() != m || degree == 0 || degree > MAX_SPLINE_DEGREE || m <= degree {
            return invalid(format!("{m} points, {} parameters, degree {degree}", params.len()));
        }
        if m > MAX_KNOTS || params.windows(2).any(|w| !(w[1] > w[0])) || params.iter().any(|t| !t.is_finite()) {
            return invalid("the parameters must be finite and increasing");
        }
        let (first, last) = (params[0], params[m - 1]);
        let mut knots = vec![first; degree + 1];
        for j in 1..m - degree {
            knots.push(params[j..j + degree].iter().sum::<f64>() / degree as f64);
        }
        knots.extend(std::iter::repeat_n(last, degree + 1));
        // N · P = Q, dense with partial pivoting (the matrix is banded; m is small here).
        let mut a = vec![vec![0.0f64; m]; m];
        for (row, &t) in a.iter_mut().zip(params) {
            let span = crate::tess::span_of(&knots, degree, m, t);
            let Some(b) = crate::tess::basis_ders(&knots, span, degree, t) else { return invalid("basis") };
            for (x, n) in row.iter_mut().skip(span - degree).zip(&b[0][..=degree]) {
                *x = *n;
            }
        }
        let mut q: Vec<[f64; N]> = points.to_vec();
        for col in 0..m {
            let Some(piv) = (col..m).max_by(|&x, &y| a[x][col].abs().total_cmp(&a[y][col].abs())) else { return invalid("empty") };
            if !(a[piv][col].abs() > 1e-14) {
                return invalid("singular interpolation system");
            }
            a.swap(col, piv);
            q.swap(col, piv);
            let (pr, pq) = (a[col].clone(), q[col]);
            for r in col + 1..m {
                let f = a[r][col] / pr[col];
                if f != 0.0 {
                    for (x, p) in a[r].iter_mut().zip(&pr).skip(col) {
                        *x -= f * p;
                    }
                    for (x, p) in q[r].iter_mut().zip(pq) {
                        *x -= f * p;
                    }
                }
            }
        }
        let mut ctrl = vec![[0.0; N]; m];
        for r in (0..m).rev() {
            let mut v = q[r];
            for (c, x) in ctrl.iter().enumerate().skip(r + 1) {
                for (vi, xi) in v.iter_mut().zip(x) {
                    *vi -= a[r][c] * xi;
                }
            }
            ctrl[r] = v.map(|x| x / a[r][r]);
        }
        if ctrl.iter().flatten().any(|x| !x.is_finite()) {
            return invalid("interpolation did not give finite control points");
        }
        Ok(CurveN { degree, knots, ctrl })
    }

    /// The point and first derivative at `t` (clamped to the curve's range).
    pub fn eval(&self, t: f64) -> Option<([f64; N], [f64; N])> {
        let (p, n) = (self.degree, self.ctrl.len());
        if n <= p || self.knots.len() != n + p + 1 || !t.is_finite() {
            return None;
        }
        let t = t.clamp(*self.knots.get(p)?, *self.knots.get(n)?);
        let span = crate::tess::span_of(&self.knots, p, n, t);
        let b = crate::tess::basis_ders(&self.knots, span, p, t)?;
        let (mut x, mut dx) = ([0.0; N], [0.0; N]);
        let ctrl = self.ctrl.get(span - p..=span)?;
        for ((c, n), dn) in ctrl.iter().zip(&b[0]).zip(&b[1]) {
            for ((xk, dk), ck) in x.iter_mut().zip(dx.iter_mut()).zip(c) {
                *xk += n * ck;
                *dk += dn * ck;
            }
        }
        Some((x, dx))
    }
}

#[cfg(test)]
mod curve_n_tests {
    use super::*;

    #[test]
    fn interpolates_its_points_and_their_slope() {
        // A helix with a fourth coordinate rising linearly: the cubic passes through every
        // point and follows the curve between them.
        let at = |t: f64| [t.cos(), t.sin(), 0.3 * t, 2.0 * t];
        let params: Vec<f64> = (0..=24).map(|k| f64::from(k) * 0.25).collect();
        let points: Vec<[f64; 4]> = params.iter().map(|&t| at(t)).collect();
        let Ok(c) = CurveN::interpolate(&params, &points, 3) else { panic!("interpolates") };
        for (t, p) in params.iter().zip(&points) {
            let Some((q, _)) = c.eval(*t) else { panic!("evaluates") };
            assert!(q.iter().zip(p).all(|(a, b)| (a - b).abs() < 1e-9), "{t}");
        }
        let Some((q, d)) = c.eval(2.6) else { panic!("evaluates") };
        let w = at(2.6);
        assert!(q.iter().zip(w).all(|(a, b)| (a - b).abs() < 1e-3), "{q:?} vs {w:?}");
        assert!((d[3] - 2.0).abs() < 1e-6 && (d[2] - 0.3).abs() < 1e-3, "{d:?}");
        assert!(CurveN::interpolate(&[0.0, 0.0, 1.0, 2.0], &points[..4], 3).is_err(), "repeated parameter");
    }
}
