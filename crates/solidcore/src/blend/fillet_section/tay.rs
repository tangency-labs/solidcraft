//! Truncated Taylor series in one variable, and the equal area thumbweight's area
//! integral and its implicit function along the blend: the series a station's thumbweight
//! and its s-derivatives are solved in, and the certificate's interval series of them.
//!
//! Fork: `sm_Tay`, `sm_TayOf`, its `operator+ - * /`, `sm_Wide`, `sm_Acos`, `sm_SinCos`,
//! `sm_TaySqrt`, `sm_TaySinCos`, `sm_PowN`, `sm_GnAreaIntegrand`, `sm_GnAreaSum`,
//! `sm_GnWedgeCos`, `sm_GnHalfAngle`, `sm_GnTarget`, `sm_GnInvert` and `sm_GnAreaCells` in
//! `source/SMLib/src/SmFilletSection.cpp` (tangency-labs/smlib@7102bc5); see [`super`] for
//! the table.

use core::ops::{Add, Div, Mul, Sub};

use super::interval::Ivl;
use super::series::{GnScalar, VSer};

/// `SM_GN_TAY`: the order the series are truncated at.
pub const TAY: usize = 4;

/// `SM_GN_AREA_N`: the composite Simpson rule's number of intervals.
pub const AREA_N: i32 = 2048;

/// The ring `Z` of the fork's templates: `double`, `sm_Ivl` and series over either.
pub trait Ring:
    Copy + Add<Output = Self> + Sub<Output = Self> + Mul<Output = Self> + Div<Output = Self>
{
    /// `Z(double)`.
    fn lift(d: f64) -> Self;
}

impl Ring for f64 {
    fn lift(d: f64) -> Self {
        d
    }
}

impl Ring for Ivl {
    fn lift(d: f64) -> Self {
        Ivl::from(d)
    }
}

/// The scalars a series' functions are taken in (`double`, `sm_Ivl`).
pub trait TayScalar: Ring {
    /// `sm_Sqrt`.
    #[must_use]
    fn sqrt_s(self) -> Self;
    /// `sm_Acos`.
    #[must_use]
    fn acos_s(self) -> Self;
    /// `sm_SinCos`: `(sin, cos)`.
    fn sin_cos_s(self) -> (Self, Self);
}

/// `smos_Min(a, b)`: `(a < b) ? a : b`.
#[inline]
fn smin(a: f64, b: f64) -> f64 {
    if a < b {
        a
    } else {
        b
    }
}

/// `smos_Max(a, b)`: `(a > b) ? a : b`.
#[inline]
fn smax(a: f64, b: f64) -> f64 {
    if a > b {
        a
    } else {
        b
    }
}

/// `sm_Acos(double)`: the arc cosine of x clamped to [-1, 1].
#[must_use]
pub fn acos_clamped(x: f64) -> f64 {
    smax(-1.0, smin(1.0, x)).acos()
}

impl TayScalar for f64 {
    fn sqrt_s(self) -> Self {
        crate::geom::vector3d::sqrt(self)
    }
    fn acos_s(self) -> Self {
        acos_clamped(self)
    }
    fn sin_cos_s(self) -> (Self, Self) {
        (self.sin(), self.cos())
    }
}

/// `sm_Wide`: `[a, b]` widened by `1e-15` relative (the C library's `acos`, `sin` and
/// `cos` are within an ulp or two), or the whole line if either is NaN.
#[must_use]
pub fn wide(a: f64, b: f64) -> Ivl {
    if a.is_nan() || b.is_nan() {
        return Ivl::whole();
    }
    Ivl::new(
        a - (a.abs() * 1.0e-15 + 1.0e-300),
        b + (b.abs() * 1.0e-15 + 1.0e-300),
    )
}

impl TayScalar for Ivl {
    fn sqrt_s(self) -> Self {
        self.sqrt()
    }
    fn acos_s(self) -> Self {
        let inside = self.lo >= -1.0 && self.hi <= 1.0;
        if !inside {
            return Ivl::whole();
        }
        wide(self.hi.acos(), self.lo.acos())
    }
    /// On `[0, pi/2]` (the double below pi/2 its top), where sin rises and cos falls; the
    /// whole line elsewhere.
    fn sin_cos_s(self) -> (Self, Self) {
        let inside = self.lo >= 0.0 && self.hi <= core::f64::consts::FRAC_PI_2;
        if !inside {
            return (Ivl::whole(), Ivl::whole());
        }
        (
            wide(self.lo.sin(), self.hi.sin()),
            wide(self.hi.cos(), self.lo.cos()),
        )
    }
}

/// `sm_Tay`: a series truncated at `x^TAY`, `c[i]` the coefficient of `x^i`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tay<S> {
    /// The coefficients.
    pub c: [S; TAY + 1],
}

impl<S: Ring> Tay<S> {
    /// `sm_Tay()`: zero.
    #[must_use]
    pub fn zero() -> Self {
        Self {
            c: [S::lift(0.0); TAY + 1],
        }
    }

    /// `sm_TayOf(x0, x1)`: `x0 + x1 x`.
    #[must_use]
    pub fn of_terms(x0: S, x1: S) -> Self {
        let mut r = Self::zero();
        r.set(0, x0);
        r.set(1, x1);
        r
    }

    /// The coefficient of `x^i` (0 past the truncation).
    #[must_use]
    pub fn at(&self, i: usize) -> S {
        self.c.get(i).copied().unwrap_or_else(|| S::lift(0.0))
    }

    /// Sets the coefficient of `x^i` (none past the truncation).
    pub fn set(&mut self, i: usize, v: S) {
        if let Some(x) = self.c.get_mut(i) {
            *x = v;
        }
    }
}

impl<S: Ring> Ring for Tay<S> {
    /// `sm_Tay(double d)`: the constant d.
    fn lift(d: f64) -> Self {
        let mut r = Self::zero();
        r.set(0, S::lift(d));
        r
    }
}

impl<S: Ring> Add for Tay<S> {
    type Output = Self;
    fn add(self, b: Self) -> Self {
        let mut r = Self::zero();
        for (ri, (&ai, &bi)) in r.c.iter_mut().zip(self.c.iter().zip(&b.c)) {
            *ri = ai + bi;
        }
        r
    }
}

impl<S: Ring> Sub for Tay<S> {
    type Output = Self;
    fn sub(self, b: Self) -> Self {
        let mut r = Self::zero();
        for (ri, (&ai, &bi)) in r.c.iter_mut().zip(self.c.iter().zip(&b.c)) {
            *ri = ai - bi;
        }
        r
    }
}

impl<S: Ring> Mul for Tay<S> {
    type Output = Self;
    fn mul(self, b: Self) -> Self {
        let mut r = Self::zero();
        for i in 0..=TAY {
            let mut sum = S::lift(0.0);
            for k in 0..=i {
                sum = sum + self.at(k) * b.at(i - k);
            }
            r.set(i, sum);
        }
        r
    }
}

impl<S: Ring> Div for Tay<S> {
    type Output = Self;
    /// `q_i = (a_i - sum_{k=1..i} b_k q_(i-k)) / b_0` (an interval `b_0` holding 0 gives
    /// the whole line).
    fn div(self, b: Self) -> Self {
        let mut r = Self::zero();
        for i in 0..=TAY {
            let mut sum = self.at(i);
            for k in 1..=i {
                sum = sum - b.at(k) * r.at(i - k);
            }
            r.set(i, sum / b.at(0));
        }
        r
    }
}

/// `sm_TaySqrt`: `r_0 = sqrt(y_0)`, `r_i = (y_i - sum_{k=1..i-1} r_k r_(i-k)) / (2 r_0)`.
#[must_use]
pub fn tay_sqrt<S: TayScalar>(y: &Tay<S>) -> Tay<S> {
    let mut r = Tay::zero();
    r.set(0, y.at(0).sqrt_s());
    for i in 1..=TAY {
        let mut sum = y.at(i);
        for k in 1..i {
            sum = sum - r.at(k) * r.at(i - k);
        }
        r.set(i, sum / (S::lift(2.0) * r.at(0)));
    }
    r
}

/// `sm_TaySinCos`: `sin u` and `cos u`, coefficient by coefficient from
/// `(sin u)' = cos u u'`, `(cos u)' = -sin u u'`.
#[must_use]
pub fn tay_sin_cos<S: TayScalar>(u: &Tay<S>) -> (Tay<S>, Tay<S>) {
    let mut rs = Tay::zero();
    let mut rc = Tay::zero();
    let (s0, c0) = u.at(0).sin_cos_s();
    rs.set(0, s0);
    rc.set(0, c0);
    for i in 1..=TAY {
        let mut a = S::lift(0.0);
        let mut b = S::lift(0.0);
        for k in 1..=i {
            a = a + S::lift(k as f64) * u.at(k) * rc.at(i - k);
            b = b + S::lift(k as f64) * u.at(k) * rs.at(i - k);
        }
        rs.set(i, a / S::lift(i as f64));
        rc.set(i, (S::lift(0.0) - b) / S::lift(i as f64));
    }
    (rs, rc)
}

/// `sm_PowN`: `a^n`, multiplied up from 1.
#[must_use]
pub fn pow_n<Z: Ring>(a: Z, n: usize) -> Z {
    let mut r = Z::lift(1.0);
    for _ in 0..n {
        r = r * a;
    }
    r
}

/// `sm_GnAreaIntegrand`: `a b' - a' b` of the order-n section between two planes at t,
/// `(1 - w)^2 t^(n+1) (1 - t)^(n+1) ((n + 2) d1 d2 + t (1 - t) w (2 - w)) / (D^2 d1^2 d2^2)`,
/// `D = (1 - t)^(n+1) + t^(n+1)`, `d1 = 1 - w + w t`, `d2 = 1 - w t`.
#[must_use]
pub fn gn_area_integrand<Z: Ring>(n: usize, t: f64, w: Z) -> Z {
    let one = Z::lift(1.0);
    let t = Z::lift(t);
    let u = one - t;
    let tn = pow_n(t, n + 1);
    let un = pow_n(u, n + 1);
    let d = un + tn;
    let d1 = one - w + w * t;
    let d2 = one - w * t;
    let v = one - w;
    let num =
        v * v * tn * un * (Z::lift((n + 2) as f64) * d1 * d2 + t * u * w * (Z::lift(2.0) - w));
    num / (d * d * d1 * d1 * d2 * d2)
}

/// `sm_GnAreaSum`: the area integral `I(w)` of the order-n section (twice the area it
/// takes off between two planes, over `|e1 x e2|`), by composite Simpson on
/// [`AREA_N`] intervals; a series in w gives its w-derivatives.
#[must_use]
pub fn gn_area_sum<Z: Ring>(n: usize, w: Z) -> Z {
    let mut sum = Z::lift(0.0);
    for i in 0..=AREA_N {
        let c = if i == 0 || i == AREA_N {
            1.0
        } else if i % 2 != 0 {
            4.0
        } else {
            2.0
        };
        sum = sum + Z::lift(c) * gn_area_integrand(n, f64::from(i) / f64::from(AREA_N), w);
    }
    sum / Z::lift(3.0 * f64::from(AREA_N))
}

/// `sm_GnWedgeCos`: the cosine of the angle between the ball's radii to its contacts,
/// `x = (F1 - V).(F2 - V) / (r1 r2)`, as a series in s from the feet's and V's
/// s-coefficients to `order` (their eps order 0).
#[must_use]
pub fn gn_wedge_cos<S: Ring + GnScalar>(
    f: &[VSer<S>; 2],
    v: &VSer<S>,
    rr: f64,
    order: usize,
) -> Tay<S> {
    let mut x = Tay::zero();
    for c in 0..3 {
        let mut a = [Tay::<S>::zero(); 2];
        for (af, ff) in a.iter_mut().zip(f) {
            let (Some(fc), Some(vc)) = (ff.a.get(c), v.a.get(c)) else {
                continue;
            };
            let mut i = 0;
            while i <= order && i <= TAY {
                af.set(i, fc.at(i, 0) - vc.at(i, 0));
                i += 1;
            }
        }
        let [a0, a1] = a;
        x = x + a0 * a1;
    }
    let inv = S::lift(1.0) / S::lift(rr);
    for xi in &mut x.c {
        *xi = *xi * inv;
    }
    x
}

/// `sm_GnHalfAngle`: `u = acos(x) / 2`, `u' = -x' / (2 sqrt(1 - x^2))`.
#[must_use]
pub fn gn_half_angle<S: TayScalar>(x: &Tay<S>) -> Tay<S> {
    let q = tay_sqrt(&(Tay::lift(1.0) - *x * *x));
    let mut dx = Tay::zero();
    for i in 0..TAY {
        dx.set(i, x.at(i + 1) * S::lift((i + 1) as f64));
    }
    let v = dx / q;
    let mut u = Tay::zero();
    u.set(0, x.at(0).acos_s() * S::lift(0.5));
    for i in 1..=TAY {
        u.set(i, (S::lift(0.0) - v.at(i - 1)) / S::lift(2.0 * i as f64));
    }
    u
}

/// `sm_GnTarget`: the area integral the ball's circle takes off for half the angle u
/// between its radii, `(sin u - u cos u) / sin^3 u`.
#[must_use]
pub fn gn_target<S: TayScalar>(u: &Tay<S>) -> Tay<S> {
    let (s, c) = tay_sin_cos(u);
    (s - *u * c) / (s * s * s)
}

/// `sm_GnInvert`: `w(s)` from `I(w(s)) = Tg(s)`, `w(s)` in `w0`: `Tg_i` is the coefficient
/// of `sigma^i` in `sum_m d_m dw^m`, `d_m = I^(m)(w(s)) / m!` (held by `d` over `w0`), in
/// which `w_i` enters only through `d_1 w_i`.
#[must_use]
pub fn gn_invert<S: Ring>(tg: &Tay<S>, d: &Tay<S>, w0: S) -> Tay<S> {
    let mut dw = Tay::zero();
    for i in 1..=TAY {
        let mut p = S::lift(0.0);
        let mut pow = dw;
        for m in 2..=i {
            pow = pow * dw;
            p = p + d.at(m) * pow.at(i);
        }
        dw.set(i, (tg.at(i) - p) / d.at(1));
    }
    dw.set(0, w0);
    dw
}

/// `sm_GnAreaCells`: enclosures of I's Taylor coefficients in w over intervals of w, on a
/// grid of cells over an edge's thumbweights, each computed once: cell k's series holds
/// `I^(m)(w) / m!` for every w in it, and the hull of the cells an interval meets holds
/// them over that interval. An interval off the grid is taken on its own.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GnAreaCells {
    /// `lOrder`: n.
    pub order: usize,
    /// `sE`: the cells' ends.
    pub ends: Vec<f64>,
    /// `sHave`, `sI`: the cells' series, once computed.
    pub series: Vec<Option<Tay<Ivl>>>,
}

impl GnAreaCells {
    /// `Init`: `cells` cells over `[lo, hi]`.
    pub fn init(&mut self, n: usize, lo: f64, hi: f64, cells: u16) {
        self.order = n;
        self.ends.clear();
        for k in 0..=cells {
            self.ends
                .push(lo + (hi - lo) * f64::from(k) / f64::from(cells));
        }
        self.series = vec![None; usize::from(cells)];
    }

    /// `Over`: the series of I in w over the interval `w`.
    pub fn over(&mut self, w: Ivl) -> Tay<Ivl> {
        let cells = self.ends.len().saturating_sub(1);
        let (Some(&front), Some(&back)) = (self.ends.first(), self.ends.last()) else {
            return gn_area_sum(self.order, Tay::of_terms(w, Ivl::from(1.0)));
        };
        if cells < 1 || !(w.lo >= front && w.hi <= back) {
            return gn_area_sum(self.order, Tay::of_terms(w, Ivl::from(1.0)));
        }
        // std::upper_bound / std::lower_bound, less one, clamped to the cells
        let ub = self.ends.partition_point(|&e| e <= w.lo);
        let lb = self.ends.partition_point(|&e| e < w.hi);
        let top = cells - 1;
        let k0 = ub.saturating_sub(1).min(top);
        let k1 = lb.saturating_sub(1).min(top).max(k0);
        let mut r = Tay::<Ivl>::zero();
        for k in k0..=k1 {
            let (Some(&e0), Some(&e1)) = (self.ends.get(k), self.ends.get(k + 1)) else {
                continue;
            };
            let order = self.order;
            let Some(slot) = self.series.get_mut(k) else {
                continue;
            };
            let sk = *slot.get_or_insert_with(|| {
                gn_area_sum(order, Tay::of_terms(Ivl::new(e0, e1), Ivl::from(1.0)))
            });
            for (rm, &sm) in r.c.iter_mut().zip(&sk.c) {
                *rm = if k == k0 { sm } else { rm.hull(sm) };
            }
        }
        r
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn series_ring_and_functions() {
        // (1 + x)^2 / (1 + x) = 1 + x
        let a = Tay::of_terms(1.0, 1.0);
        let q = (a * a) / a;
        assert!((q.c[0] - 1.0).abs() < 1e-15 && (q.c[1] - 1.0).abs() < 1e-15);
        assert!(q.c[2..].iter().all(|c| c.abs() < 1e-15));
        // sqrt(1 + x): 1, 1/2, -1/8, 1/16, -5/128
        let r = tay_sqrt(&a);
        for (c, e) in r.c.iter().zip([1.0, 0.5, -0.125, 0.0625, -5.0 / 128.0]) {
            assert!((c - e).abs() < 1e-15, "{c} {e}");
        }
        // sin x, cos x at 0
        let (s, c) = tay_sin_cos(&Tay::of_terms(0.0, 1.0));
        for (x, e) in s.c.iter().zip([0.0, 1.0, 0.0, -1.0 / 6.0, 0.0]) {
            assert!((x - e).abs() < 1e-15);
        }
        for (x, e) in c.c.iter().zip([1.0, 0.0, -0.5, 0.0, 1.0 / 24.0]) {
            assert!((x - e).abs() < 1e-15);
        }
    }

    /// The right angle's target, u = pi/4: (sin u - u cos u) / sin^3 u, against the old
    /// cot form (cot(a/2) - (pi - a)/2) / (cot(a/2) cos^2(a/2)) at a = pi/2.
    #[test]
    fn target_is_the_cot_form() {
        for a in [0.3_f64, 1.0, core::f64::consts::FRAC_PI_2, 2.5] {
            let u = 0.5 * (core::f64::consts::PI - a);
            let t = gn_target(&Tay::<f64>::lift(u)).c[0];
            let h = 0.5 * a;
            let cot = h.cos() / h.sin();
            let e = (cot - 0.5 * (core::f64::consts::PI - a)) / (cot * h.cos() * h.cos());
            assert!((t - e).abs() < 1e-13 * e.abs(), "{a} {t} {e}");
        }
    }

    /// The closed-form integrand sums to the integral of `a b' - a' b`: w = 0 is the
    /// section with no thumbweight; the derivative series agrees with a difference.
    #[test]
    fn area_sum_and_its_derivative() {
        for n in 2..=4 {
            let i0 = gn_area_sum(n, -0.3_f64);
            let s = gn_area_sum(n, Tay::of_terms(-0.3_f64, 1.0));
            assert!((s.c[0] - i0).abs() < 1e-15);
            let h = 1e-6;
            let fd = (gn_area_sum(n, -0.3 + h) - gn_area_sum(n, -0.3 - h)) / (2.0 * h);
            assert!((s.c[1] - fd).abs() < 1e-7, "{n} {} {fd}", s.c[1]);
            assert!(s.c[1] < 0.0);
            // the interval sum holds the double one
            let iv = gn_area_sum(n, Ivl::from(-0.3));
            assert!(iv.lo <= i0 && i0 <= iv.hi && iv.hi - iv.lo < 1e-9);
        }
    }

    #[test]
    fn cells_hold_the_series_over_their_cells() {
        let mut cells = GnAreaCells::default();
        cells.init(2, -1.0, 0.5, 8);
        let w = Ivl::new(-0.3, -0.2);
        let r = cells.over(w);
        let at = gn_area_sum(2, Tay::of_terms(-0.25_f64, 1.0));
        for (ri, ai) in r.c.iter().zip(&at.c) {
            assert!(ri.lo <= *ai && *ai <= ri.hi);
        }
        assert!(cells.series.iter().filter(|s| s.is_some()).count() >= 1);
        // off the grid: taken on its own
        let off = cells.over(Ivl::new(0.6, 0.7));
        assert!(off.c[0].lo <= gn_area_sum(2, 0.65) && gn_area_sum(2, 0.65) <= off.c[0].hi);
    }
}
