//! Spline function spaces and the linear operators surface construction is built from: a clamped
//! B-spline space ([`Space`]), interpolation in it with derivative (Hermite) conditions
//! ([`Interpolant`]), carrying a spline into a larger space that contains it ([`embed`]: Bézier
//! degree elevation and Boehm knot insertion, exact), and a monotone piecewise cubic through
//! increasing data ([`Monotone`], for reparameterising curves).
//!
//! These are the pieces that make several splines *compatible* (one degree, one knot vector) so
//! that their control points can be combined linearly: lofting, Coons and Gordon surfaces, and
//! cross-boundary derivative conditions all reduce to them.
//!
//! References: Piegl & Tiller, *The NURBS Book* (2nd ed.) — §2.2–2.3 (basis functions and
//! their derivatives), §5.2 (knot insertion), §5.5 (degree elevation; the Bézier case), §9.2.1
//! (global interpolation, knots by averaging the parameters) and §9.2.2 (interpolation with
//! end derivatives); Fritsch & Carlson 1980, "Monotone piecewise cubic interpolation" (SIAM J.
//! Numer. Anal. 17), with the weighted harmonic-mean slopes of Fritsch & Butland 1984.
//!
//! Input is hostile: constructors check sizes and finiteness and return [`SplineError`]; no
//! function here panics.

use crate::bspline::{HPoint, SplineError, SplineResult, invalid};
use solidcore_record::Vec3;

/// Most basis functions a space used for interpolation may have (its systems are solved densely).
pub const MAX_SPACE_DIMENSION: usize = solidcraft_kernel_services::spline_space::MAX_SPACE_DIMENSION;

/// Values a spline can carry: numbers and points (vectors).
pub trait Linear: Copy {
    fn zero() -> Self;
    fn plus(self, o: Self) -> Self;
    fn times(self, s: f64) -> Self;
    /// The value as a homogeneous point of weight one (knot insertion works on those).
    fn to_h(self) -> HPoint;
    fn from_h(h: HPoint) -> Self;
    /// Raw linear components for the kernel; unused scalar/vector slots are zero.
    fn raw_components(self) -> [f64; 4] {
        let h = self.to_h();
        [h.x, h.y, h.z, h.w]
    }
    fn is_finite(self) -> bool;
}

impl Linear for f64 {
    fn raw_components(self) -> [f64; 4] {
        [self, 0.0, 0.0, 0.0]
    }
    fn zero() -> Self {
        0.0
    }
    fn plus(self, o: Self) -> Self {
        self + o
    }
    fn times(self, s: f64) -> Self {
        self * s
    }
    fn to_h(self) -> HPoint {
        HPoint { x: self, y: 0.0, z: 0.0, w: 1.0 }
    }
    fn from_h(h: HPoint) -> Self {
        h.x
    }
    fn is_finite(self) -> bool {
        f64::is_finite(self)
    }
}

impl Linear for Vec3 {
    fn raw_components(self) -> [f64; 4] {
        [self.x, self.y, self.z, 0.0]
    }
    fn zero() -> Self {
        Vec3::ZERO
    }
    fn plus(self, o: Self) -> Self {
        self + o
    }
    fn times(self, s: f64) -> Self {
        self * s
    }
    fn to_h(self) -> HPoint {
        HPoint::weighted(self, 1.0)
    }
    fn from_h(h: HPoint) -> Self {
        h.xyz()
    }
    fn is_finite(self) -> bool {
        Vec3::is_finite(self)
    }
}

/// Homogeneous points as they are: a rational spline's control values `(x·w, y·w, z·w, w)`
/// combine linearly, so carrying one into a larger space (degree elevation, knot insertion) is
/// exact on them.
impl Linear for HPoint {
    fn zero() -> Self {
        HPoint::ZERO
    }
    fn plus(self, o: Self) -> Self {
        self + o
    }
    fn times(self, s: f64) -> Self {
        self * s
    }
    fn to_h(self) -> HPoint {
        self
    }
    fn from_h(h: HPoint) -> Self {
        h
    }
    fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite() && self.w.is_finite()
    }
}

pub(crate) fn failure(error: solidcraft_kernel_services::spline_space::SplineFailure) -> SplineError {
    use solidcraft_kernel_services::spline_space::SplineFailure;
    match error {
        SplineFailure::InvalidInput => SplineError::Invalid("spline-space arguments are invalid".into()),
        SplineFailure::InvalidKnots => SplineError::Invalid("the knot spaces are invalid or incompatible".into()),
        SplineFailure::InvalidCount => SplineError::Invalid("control, condition, or matrix dimensions do not match".into()),
        SplineFailure::InvalidParameter => SplineError::Invalid("an interpolation parameter is invalid".into()),
        SplineFailure::Singular => SplineError::Invalid("the interpolation conditions do not determine a spline".into()),
        SplineFailure::NonFinite => SplineError::Invalid("spline-space calculation did not give finite values".into()),
        SplineFailure::UnsupportedDegree => SplineError::Unsupported("degree is unsupported for the requested spline-space operation".into()),
        SplineFailure::TooLarge => SplineError::TooLarge("spline-space operation exceeds its dimension bound".into()),
        SplineFailure::Allocation => SplineError::TooLarge("spline-space storage could not be allocated".into()),
    }
}
fn raw<T: Linear>(values: &[T]) -> Vec<[f64; 4]> {
    values.iter().map(|v| v.raw_components()).collect()
}
fn value<T: Linear>([x, y, z, w]: [f64; 4]) -> T {
    T::from_h(HPoint { x, y, z, w })
}

/// Opaque clamped space; numerical operators are owned by SolidCore.
#[derive(Clone, Debug, PartialEq)]
pub struct Space(solidcraft_kernel_services::spline_space::Space);
impl Space {
    pub fn new(degree: usize, knots: Vec<f64>) -> SplineResult<Self> {
        solidcraft_kernel_services::spline_space::Space::new(degree, knots).map(Self).map_err(failure)
    }
    pub fn polynomial(degree: usize, a: f64, b: f64) -> SplineResult<Self> {
        solidcraft_kernel_services::spline_space::Space::polynomial(degree, a, b).map(Self).map_err(failure)
    }
    pub fn with_breaks(degree: usize, breaks: &[f64]) -> SplineResult<Self> {
        solidcraft_kernel_services::spline_space::Space::with_breaks(degree, breaks).map(Self).map_err(failure)
    }
    pub fn degree(&self) -> usize {
        self.0.degree()
    }
    pub fn knots(&self) -> &[f64] {
        self.0.knots()
    }
    pub fn count(&self) -> usize {
        self.0.count()
    }
    pub fn domain(&self) -> [f64; 2] {
        self.0.domain()
    }
    pub fn basis(&self, t: f64, der: usize) -> SplineResult<(usize, Vec<f64>)> {
        self.0.basis(t, der).map_err(failure)
    }
    pub fn basis_left(&self, t: f64, der: usize) -> SplineResult<(usize, Vec<f64>)> {
        self.0.basis_left(t, der).map_err(failure)
    }
    pub fn eval<T: Linear>(&self, ctrl: &[T], t: f64, der: usize) -> SplineResult<T> {
        self.0.eval(ctrl, t, der, |p| p.raw_components()).map(value).map_err(failure)
    }
    pub fn refined(&self, breaks: &[f64]) -> SplineResult<Self> {
        self.0.refined(breaks).map(Self).map_err(failure)
    }
}
pub use solidcraft_kernel_services::spline_space::Cond;
/// A factored interpolation operator retaining the application's borrowed-space interface.
#[derive(Clone, Debug)]
pub struct Interpolant {
    space: Space,
    operator: solidcraft_kernel_services::spline_space::Interpolant,
}
impl Interpolant {
    pub fn new(space: Space, conds: Vec<Cond>) -> SplineResult<Self> {
        let operator = solidcraft_kernel_services::spline_space::Interpolant::new(space.0.clone(), conds).map_err(failure)?;
        Ok(Self { space, operator })
    }
    pub fn lowest(conds: Vec<Cond>, a: f64, b: f64) -> SplineResult<Self> {
        let operator = solidcraft_kernel_services::spline_space::Interpolant::lowest(conds, a, b).map_err(failure)?;
        let space = Space(operator.space());
        Ok(Self { space, operator })
    }
    pub fn space(&self) -> &Space {
        &self.space
    }
    pub fn conds(&self) -> &[Cond] {
        self.operator.conds()
    }
    pub fn fit<T: Linear>(&self, data: &[T]) -> SplineResult<Vec<T>> {
        if data.len() != self.conds().len() {
            return Err(failure(solidcraft_kernel_services::spline_space::SplineFailure::InvalidCount));
        }
        self.operator.fit(&raw(data)).map(|v| v.into_iter().map(value).collect()).map_err(failure)
    }
}
/// Carry values into an exactly containing spline space through the kernel.
pub fn embed<T: Linear>(from: &Space, ctrl: &[T], to: &Space) -> SplineResult<Vec<T>> {
    if ctrl.len() != from.count() {
        return Err(failure(solidcraft_kernel_services::spline_space::SplineFailure::InvalidCount));
    }
    solidcraft_kernel_services::spline_space::embed(&from.0, &raw(ctrl), &to.0).map(|v| v.into_iter().map(value).collect()).map_err(failure)
}

/// A monotone piecewise cubic Hermite function through `(xₖ, yₖ)`, both strictly increasing
/// (Fritsch & Carlson): slopes are the weighted harmonic means of the neighbouring secants
/// (Fritsch & Butland), the secant at the ends. Its slope is positive everywhere, and through two
/// points it is the straight line.
#[derive(Clone, Debug, PartialEq)]
pub struct Monotone {
    x: Vec<f64>,
    y: Vec<f64>,
    m: Vec<f64>,
}

impl Monotone {
    pub fn new(x: Vec<f64>, y: Vec<f64>) -> SplineResult<Monotone> {
        let n = x.len();
        if n < 2 || y.len() != n {
            return invalid("a monotone cubic needs two points or more");
        }
        let increasing = |v: &[f64]| v.iter().all(|t| t.is_finite()) && v.windows(2).all(|w| w[0] < w[1]);
        if !increasing(&x) || !increasing(&y) {
            return invalid("the points of a monotone cubic must increase");
        }
        let h: Vec<f64> = x.windows(2).map(|w| w[1] - w[0]).collect();
        let d: Vec<f64> = y.windows(2).zip(&h).map(|(w, h)| (w[1] - w[0]) / h).collect();
        let mut m = Vec::with_capacity(n);
        m.push(d[0]);
        for k in 1..n - 1 {
            let (w1, w2) = (2.0 * h[k] + h[k - 1], h[k] + 2.0 * h[k - 1]);
            m.push((w1 + w2) / (w1 / d[k - 1] + w2 / d[k]));
        }
        m.push(d[n - 2]);
        Ok(Monotone { x, y, m })
    }

    /// The value and the first two derivatives at `t` (clamped to the points' range).
    pub fn eval(&self, t: f64) -> [f64; 3] {
        let n = self.x.len();
        let t = t.clamp(self.x[0], self.x[n - 1]);
        let k = self.x.partition_point(|x| *x <= t).saturating_sub(1).min(n - 2);
        let (x0, x1, y0, y1, m0, m1) = (self.x[k], self.x[k + 1], self.y[k], self.y[k + 1], self.m[k], self.m[k + 1]);
        let h = x1 - x0;
        let s = (t - x0) / h;
        let (s2, s3) = (s * s, s * s * s);
        let value = (2.0 * s3 - 3.0 * s2 + 1.0) * y0 + (s3 - 2.0 * s2 + s) * h * m0 + (-2.0 * s3 + 3.0 * s2) * y1 + (s3 - s2) * h * m1;
        let d1 = ((6.0 * s2 - 6.0 * s) * y0 + (3.0 * s2 - 4.0 * s + 1.0) * h * m0 + (-6.0 * s2 + 6.0 * s) * y1 + (3.0 * s2 - 2.0 * s) * h * m1) / h;
        let d2 = ((12.0 * s - 6.0) * y0 + (6.0 * s - 4.0) * h * m0 + (-12.0 * s + 6.0) * y1 + (6.0 * s - 2.0) * h * m1) / (h * h);
        [value, d1, d2]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prelude::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-10
    }

    /// Hermite interpolation reproduces a cubic from its values and end derivatives, and the
    /// lowest-degree interpolant through two values is the straight line.
    #[test]
    fn interpolation_meets_values_and_derivatives() {
        let f = |t: f64| 1.0 + 2.0 * t - 3.0 * t * t + 0.5 * t * t * t;
        let df = |t: f64| 2.0 - 6.0 * t + 1.5 * t * t;
        let d2f = |t: f64| -6.0 + 3.0 * t;
        let conds = vec![Cond { t: 0.0, der: 0 }, Cond { t: 0.0, der: 1 }, Cond { t: 0.0, der: 2 }, Cond { t: 2.0, der: 0 }];
        let it = Interpolant::lowest(conds, 0.0, 2.0).unwrap();
        let c = it.fit(&[f(0.0), df(0.0), d2f(0.0), f(2.0)]).unwrap();
        for k in 0..=10 {
            let t = k as f64 * 0.2;
            assert!(close(it.space().eval(&c, t, 0).unwrap(), f(t)), "{t}");
        }
        let line = Interpolant::lowest(vec![Cond { t: 0.0, der: 0 }, Cond { t: 1.0, der: 0 }], 0.0, 1.0).unwrap();
        let c = line.fit(&[2.0, 5.0]).unwrap();
        assert!(close(line.space().eval(&c, 0.25, 0).unwrap(), 2.75));
        let constant = Interpolant::lowest(vec![Cond { t: 0.3, der: 0 }], 0.0, 1.0).unwrap();
        let c = constant.fit(&[4.0, 0.0]).unwrap();
        assert!(close(constant.space().eval(&c, 0.9, 0).unwrap(), 4.0));
    }

    /// Many conditions: a cubic spline with averaged knots through samples of a smooth function
    /// with derivative conditions at both ends (values exact at the samples).
    #[test]
    fn spline_interpolation_with_end_derivatives() {
        let f = |t: f64| (2.0 * t).sin();
        let df = |t: f64| 2.0 * (2.0 * t).cos();
        let ts: Vec<f64> = (0..=8).map(|k| k as f64 / 8.0).collect();
        let mut conds: Vec<Cond> = ts.iter().map(|&t| Cond { t, der: 0 }).collect();
        conds.push(Cond { t: 0.0, der: 1 });
        conds.push(Cond { t: 1.0, der: 1 });
        let it = Interpolant::lowest(conds, 0.0, 1.0).unwrap();
        let mut data: Vec<f64> = ts.iter().map(|&t| f(t)).collect();
        data.push(df(0.0));
        data.push(df(1.0));
        let c = it.fit(&data).unwrap();
        for &t in &ts {
            assert!(close(it.space().eval(&c, t, 0).unwrap(), f(t)));
        }
        assert!(close(it.space().eval(&c, 0.0, 1).unwrap(), df(0.0)));
        assert!(close(it.space().eval(&c, 1.0, 1).unwrap(), df(1.0)));
        assert!((it.space().eval(&c, 0.53, 0).unwrap() - f(0.53)).abs() < 1e-4);
    }

    /// Embedding is exact: a quadratic polynomial raised to a cubic with knots inserted, and a
    /// cubic spline refined, evaluate the same everywhere; a space that does not contain the
    /// spline is refused.
    #[test]
    fn embedding_keeps_the_spline() {
        let from = Space::polynomial(2, 0.0, 1.0).unwrap();
        let ctrl = [Vec3::new(0.0, 0.0, 0.0), Vec3::new(1.0, 2.0, 0.0), Vec3::new(3.0, -1.0, 1.0)];
        let to = Space::with_breaks(3, &[0.0, 0.2, 0.5, 0.7, 1.0]).unwrap();
        let out = embed(&from, &ctrl, &to).unwrap();
        let spline = Space::with_breaks(3, &[0.0, 0.5, 1.0]).unwrap();
        let sctrl: Vec<f64> = (0..spline.count()).map(|i| (i as f64 * 1.7).sin()).collect();
        let finer = spline.refined(&[0.25, 0.5, 0.8]).unwrap();
        let sout = embed(&spline, &sctrl, &finer).unwrap();
        for k in 0..=20 {
            let t = k as f64 / 20.0;
            assert!(from.eval(&ctrl, t, 0).unwrap().dist(to.eval(&out, t, 0).unwrap()) < 1e-12);
            assert!(close(spline.eval(&sctrl, t, 0).unwrap(), finer.eval(&sout, t, 0).unwrap()));
        }
        assert!(embed(&finer, &sout, &spline).is_err(), "a coarser space");
        assert!(embed(&spline, &sctrl, &Space::with_breaks(3, &[0.0, 0.4, 1.0]).unwrap()).is_err(), "a knot missing");
        assert!(embed(&spline, &sctrl, &Space::with_breaks(3, &[0.0, 0.5, 2.0]).unwrap()).is_err(), "another domain");
    }

    #[test]
    fn monotone_cubic_is_monotone_and_straight_through_two_points() {
        let m = Monotone::new(vec![0.0, 0.3, 0.5, 1.0], vec![0.0, 0.05, 0.9, 1.0]).unwrap();
        let mut last = -1.0;
        for k in 0..=200 {
            let [y, d, _] = m.eval(k as f64 / 200.0);
            assert!(y >= last && d >= 0.0, "{k}");
            last = y;
        }
        assert!(close(m.eval(0.3)[0], 0.05) && close(m.eval(0.5)[0], 0.9));
        let line = Monotone::new(vec![0.0, 1.0], vec![2.0, 4.0]).unwrap();
        assert!(close(line.eval(0.25)[0], 2.5) && close(line.eval(0.7)[1], 2.0) && close(line.eval(0.7)[2], 0.0));
        assert!(Monotone::new(vec![0.0, 0.0], vec![0.0, 1.0]).is_err());
        assert!(Monotone::new(vec![0.0, 1.0], vec![1.0, 0.0]).is_err());
        assert!(Monotone::new(vec![0.0, f64::NAN], vec![0.0, 1.0]).is_err());
    }

    #[test]
    fn hostile_spaces_are_refused() {
        assert!(Space::new(3, vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0]).is_ok());
        assert!(Space::new(3, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0]).is_err(), "too few knots");
        assert!(Space::new(3, vec![0.0, 0.0, 0.0, 0.1, 1.0, 1.0, 1.0, 1.0]).is_err(), "not clamped");
        assert!(Space::new(1, vec![0.0, 0.0, 0.5, 0.5, 1.0, 1.0]).is_err(), "a discontinuous space");
        assert!(Space::new(0, vec![0.0, 1.0]).is_err());
        assert!(Space::with_breaks(3, &[0.0, 0.5, 0.5, 1.0]).is_err());
        assert!(Space::with_breaks(3, &[0.0, f64::NAN, 1.0]).is_err());
        let s = Space::with_breaks(3, &[0.0, 1.0]).unwrap();
        assert!(s.eval(&[1.0, 2.0], 0.5, 0).is_err(), "wrong control count");
        assert!(s.basis(f64::NAN, 0).is_err());
        // Repeated conditions do not determine a spline.
        assert!(Interpolant::new(s.clone(), vec![Cond { t: 0.5, der: 0 }; 4]).is_err());
        assert!(Interpolant::lowest(vec![Cond { t: 2.0, der: 0 }], 0.0, 1.0).is_err());
    }

    #[test]
    fn kernel_space_failures_keep_existing_public_error_variants() {
        assert!(matches!(Space::polynomial(usize::MAX, 0.0, 1.0), Err(SplineError::Unsupported(_))));
        assert!(matches!(Space::with_breaks(1, &vec![0.0; MAX_SPACE_DIMENSION + 1]), Err(SplineError::TooLarge(_))));
        assert!(matches!(Space::new(1, vec![0.0, 0.0, f64::NAN, 1.0]), Err(SplineError::Invalid(_))));
    }

    #[test]
    fn malformed_fit_and_embed_counts_refuse_before_converting_any_values() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static CONVERSIONS: AtomicUsize = AtomicUsize::new(0);
        #[derive(Clone, Copy)]
        struct Counted(f64);
        impl Linear for Counted {
            fn zero() -> Self {
                Self(0.0)
            }
            fn plus(self, other: Self) -> Self {
                Self(self.0 + other.0)
            }
            fn times(self, scalar: f64) -> Self {
                Self(self.0 * scalar)
            }
            fn to_h(self) -> HPoint {
                CONVERSIONS.fetch_add(1, Ordering::Relaxed);
                HPoint { x: self.0, y: 0.0, z: 0.0, w: 1.0 }
            }
            fn from_h(h: HPoint) -> Self {
                Self(h.x)
            }
            fn is_finite(self) -> bool {
                self.0.is_finite()
            }
        }
        let space = Space::polynomial(1, 0.0, 1.0).unwrap();
        let operator = Interpolant::new(space.clone(), vec![Cond { t: 0.0, der: 0 }, Cond { t: 1.0, der: 0 }]).unwrap();
        let wrong = vec![Counted(1.0); 3];
        assert!(matches!(operator.fit(&wrong), Err(SplineError::Invalid(_))));
        assert!(matches!(embed(&space, &wrong, &space), Err(SplineError::Invalid(_))));
        assert_eq!(CONVERSIONS.load(Ordering::Relaxed), 0, "shape refusals must precede raw conversion and allocation");
    }
}
