//! Clamped spline operators and deterministic confluent interpolation.
//!
//! Piegl and Tiller, The NURBS Book §§2, 5.2, 5.5 and 9.2. Ordered LU
//! reduction and first-maximum pivot ownership retain the authoring operator's
//! arithmetic. Basis evaluation and Boehm insertion reuse the existing evaluator.
use crate::error::nurbs::NurbsFailure;
use crate::eval::pcurve::find_span;
use crate::eval::plan::basis::{local_basis_array, Side};
use crate::{Result, SolidError};

/// Most basis functions in a densely factored interpolation space.
pub const MAX_SPACE_DIMENSION: usize = 2048;
const MAX_SPLINE_DEGREE: usize = crate::eval::plan::MAX_SPLINE_DEGREE;

const OP: &str = "geom::spline_space";
fn refuse<T>(reason: NurbsFailure) -> Result<T> {
    Err(SolidError::nurbs(OP, reason))
}
fn filled<T: Clone>(count: usize, value: T) -> Result<Vec<T>> {
    if count > MAX_SPACE_DIMENSION * MAX_SPACE_DIMENSION {
        return refuse(NurbsFailure::Storage);
    }
    let mut out = Vec::new();
    out.try_reserve_exact(count)
        .map_err(|_| SolidError::nurbs(OP, NurbsFailure::Memory))?;
    out.resize(count, value);
    Ok(out)
}
fn checked_swap<T: Copy>(values: &mut [T], a: usize, b: usize) -> Result<()> {
    let x = *values
        .get(a)
        .ok_or_else(|| SolidError::nurbs(OP, NurbsFailure::Index))?;
    let y = *values
        .get(b)
        .ok_or_else(|| SolidError::nurbs(OP, NurbsFailure::Index))?;
    *values
        .get_mut(a)
        .ok_or_else(|| SolidError::nurbs(OP, NurbsFailure::Index))? = y;
    *values
        .get_mut(b)
        .ok_or_else(|| SolidError::nurbs(OP, NurbsFailure::Index))? = x;
    Ok(())
}

/// Values on which the factored linear operator acts, without changing reduction order.
pub trait Linear: Copy {
    /// Additive identity.
    fn zero() -> Self;
    /// Ordered sum.
    #[must_use]
    fn plus(self, other: Self) -> Self;
    /// Ordered scalar product.
    #[must_use]
    fn times(self, scalar: f64) -> Self;
    /// Four raw components for shared homogeneous knot insertion.
    fn to_h(self) -> [f64; 4];
    /// Recover the linear value from homogeneous components.
    fn from_h(value: [f64; 4]) -> Self;
    /// Whether all components are finite.
    fn finite(self) -> bool;
}
impl Linear for [f64; 4] {
    fn zero() -> Self {
        [0.0; 4]
    }
    fn plus(self, other: Self) -> Self {
        let [a, b, c, d] = self;
        let [e, f, g, h] = other;
        [a + e, b + f, c + g, d + h]
    }
    fn times(self, scalar: f64) -> Self {
        self.map(|x| x * scalar)
    }
    fn to_h(self) -> [f64; 4] {
        self
    }
    fn from_h(value: [f64; 4]) -> Self {
        value
    }
    fn finite(self) -> bool {
        self.into_iter().all(f64::is_finite)
    }
}

/// A clamped B-spline space: a degree and a knot vector whose first and last `degree + 1` knots
/// are the domain's ends, interior knots repeated at most `degree` times (the space's splines
/// are continuous).
#[derive(Clone, Debug, PartialEq)]
pub struct Space {
    degree: usize,
    knots: Vec<f64>,
}

impl Space {
    /// Validate the bounded clamped knot space.
    pub fn new(degree: usize, knots: Vec<f64>) -> Result<Space> {
        if !(1..=MAX_SPLINE_DEGREE).contains(&degree) {
            return Err(SolidError::nurbs(OP, NurbsFailure::Degree));
        }
        let n = knots
            .len()
            .checked_sub(degree + 1)
            .filter(|&n| n > degree)
            .ok_or_else(|| SolidError::nurbs(OP, NurbsFailure::Knots))?;
        if n > MAX_SPACE_DIMENSION {
            return Err(SolidError::nurbs(OP, NurbsFailure::Storage));
        }
        let mut work = 16_384;
        crate::eval::ray_spline::Axis::validate(&knots, n, &mut work)
            .map_err(|_| SolidError::nurbs(OP, NurbsFailure::Knots))?;
        Ok(Self { degree, knots })
    }

    /// Polynomials of `degree` on `[a, b]` (one Bézier piece).
    pub fn polynomial(degree: usize, a: f64, b: f64) -> Result<Space> {
        if !(1..=MAX_SPLINE_DEGREE).contains(&degree) {
            return Err(SolidError::nurbs(OP, NurbsFailure::Degree));
        }
        let mut knots = filled(degree + 1, a)?;
        knots.extend(std::iter::repeat_n(b, degree + 1));
        Space::new(degree, knots)
    }

    /// Splines of `degree` with a simple knot at every interior break: `breaks` increase
    /// strictly, the first and last are the domain's ends.
    pub fn with_breaks(degree: usize, breaks: &[f64]) -> Result<Space> {
        let (Some(&a), Some(&b)) = (breaks.first(), breaks.last()) else {
            return refuse(NurbsFailure::Input);
        };
        if breaks.len() > MAX_SPACE_DIMENSION {
            return refuse(NurbsFailure::Storage);
        }
        if breaks.windows(2).any(|w| !matches!(w,[a,b] if a < b)) {
            return refuse(NurbsFailure::Input);
        }
        if !(1..=MAX_SPLINE_DEGREE).contains(&degree) {
            return Err(SolidError::nurbs(OP, NurbsFailure::Degree));
        }
        let mut knots = filled(degree + 1, a)?;
        knots.extend(
            breaks
                .iter()
                .skip(1)
                .take(breaks.len().saturating_sub(2))
                .copied(),
        );
        knots.extend(std::iter::repeat_n(b, degree + 1));
        Space::new(degree, knots)
    }

    /// Polynomial degree.
    pub fn degree(&self) -> usize {
        self.degree
    }

    /// Complete immutable clamped knot vector.
    pub fn knots(&self) -> &[f64] {
        &self.knots
    }

    /// The number of basis functions (control points of a spline in the space).
    pub fn count(&self) -> usize {
        self.knots.len() - self.degree - 1
    }

    /// The domain `[a, b]`.
    pub fn domain(&self) -> [f64; 2] {
        [
            self.knots.first().copied().unwrap_or(f64::NAN),
            self.knots.last().copied().unwrap_or(f64::NAN),
        ]
    }

    /// The `der`-th derivatives at `t` (clamped to the domain) of the basis functions that do not
    /// vanish there: the index of the first one and their values.
    pub fn basis(&self, t: f64, der: usize) -> Result<(usize, Vec<f64>)> {
        let [a, b] = self.domain();
        if !t.is_finite() {
            return refuse(NurbsFailure::Parameter);
        }
        let t = t.max(a).min(b);
        let (p, n) = (self.degree, self.count());
        let span = find_span(&self.knots, p, n, t)
            .ok_or_else(|| SolidError::nurbs(OP, NurbsFailure::Knots))?;
        let side = if t >= b { Side::Left } else { Side::Right };
        let first = span - p;
        let values = (first..=span).map(|i| {
            self.knots.get(i..i + p + 2).map_or(0.0, |k| {
                local_basis_array(k, t, side, der.min(MAX_SPLINE_DEGREE))
                    .get(der)
                    .copied()
                    .unwrap_or(0.0)
            })
        });
        Ok((first, values.collect()))
    }

    /// [`Space::basis`] from the left of `t`: at an interior knot, the derivatives of the
    /// polynomial piece that ends there (elsewhere the same as from the right).
    pub fn basis_left(&self, t: f64, der: usize) -> Result<(usize, Vec<f64>)> {
        let [a, b] = self.domain();
        if !t.is_finite() {
            return refuse(NurbsFailure::Parameter);
        }
        let t = t.max(a).min(b);
        if t <= a {
            return self.basis(t, der);
        }
        let p = self.degree;
        // The span [k_s, k_{s+1}) with k_s < t ≤ k_{s+1}.
        let below = self.knots.partition_point(|&x| x < t);
        let span = below
            .checked_sub(1)
            .filter(|s| *s >= p)
            .ok_or_else(|| SolidError::nurbs(OP, NurbsFailure::Knots))?;
        let first = span - p;
        let values = (first..=span).map(|i| {
            self.knots.get(i..i + p + 2).map_or(0.0, |k| {
                local_basis_array(k, t, Side::Left, der.min(MAX_SPLINE_DEGREE))
                    .get(der)
                    .copied()
                    .unwrap_or(0.0)
            })
        });
        Ok((first, values.collect()))
    }

    /// The `der`-th derivative at `t` of the spline with control values `ctrl`.
    pub fn eval<T: Linear>(&self, ctrl: &[T], t: f64, der: usize) -> Result<T> {
        self.eval_mapped(ctrl, t, der, |v| v.to_h()).map(T::from_h)
    }

    /// Evaluate only local controls through a representation callback, without copying a net.
    pub fn eval_mapped<C>(
        &self,
        ctrl: &[C],
        t: f64,
        der: usize,
        value: impl Fn(&C) -> [f64; 4],
    ) -> Result<[f64; 4]> {
        if ctrl.len() != self.count() {
            return refuse(NurbsFailure::CurveDefinition);
        }
        let (first, values) = self.basis(t, der)?;
        let result = values
            .iter()
            .zip(ctrl.iter().skip(first))
            .fold(<[f64; 4] as Linear>::zero(), |sum, (n, c)| {
                sum.plus(value(c).times(*n))
            });
        if !Linear::finite(result) {
            return refuse(NurbsFailure::Numerical);
        }
        Ok(result)
    }

    /// The splines of this space with `breaks` added as simple knots (those already knots are
    /// left as they are). The result contains this space.
    pub fn refined(&self, breaks: &[f64]) -> Result<Space> {
        let [a, b] = self.domain();
        if breaks.len() > MAX_SPACE_DIMENSION {
            return Err(SolidError::nurbs(OP, NurbsFailure::Storage));
        }
        let mut knots = self.knots.clone();
        for &x in breaks {
            if x > a && x < b && !knots.contains(&x) {
                knots.push(x);
            }
        }
        knots.sort_by(f64::total_cmp);
        Space::new(self.degree, knots)
    }
}

/// A condition on a spline: the value (`der` = 0) or a derivative at `t`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cond {
    /// Condition parameter.
    pub t: f64,
    /// Derivative order.
    pub der: usize,
}

/// Interpolation in a space: the spline whose values and derivatives at the conditions are given
/// (as many conditions as the space has basis functions). The system is factored once; each
/// [`Interpolant::fit`] solves it for new data, so one interpolant serves many curves and the
/// rows of a surface alike (the operator is linear).
#[derive(Clone, Debug)]
pub struct Interpolant {
    space: Space,
    conds: Vec<Cond>,
    lu: Lu,
}

impl Interpolant {
    /// Factor a determined confluent interpolation system.
    pub fn new(space: Space, conds: Vec<Cond>) -> Result<Interpolant> {
        let n = space.count();
        if conds.len() != n {
            return refuse(NurbsFailure::CurveDefinition);
        }
        let mut a = filled(n * n, 0.0)?;
        for (r, c) in conds.iter().enumerate() {
            let (first, values) = space.basis(c.t, c.der)?;
            for (k, v) in values.iter().enumerate() {
                if let Some(x) = a.get_mut(r * n + first + k) {
                    *x = *v;
                }
            }
        }
        let lu = Lu::new(n, a)?;
        Ok(Interpolant { space, conds, lu })
    }

    /// Interpolation by a single polynomial piece of the lowest degree that meets `conds` (degree
    /// one less than their number, at least one: a lone value is held constant), on `[a, b]`;
    /// with more than four conditions, a cubic spline whose interior knots average the
    /// conditions' parameters (taken as a sequence with one entry per condition, NURBS Book eq.
    /// 9.8 applied to that confluent sequence, which keeps every basis function's support over
    /// the condition it is matched with).
    pub fn lowest(conds: Vec<Cond>, a: f64, b: f64) -> Result<Interpolant> {
        let mut conds = conds;
        if conds.len() > MAX_SPACE_DIMENSION {
            return Err(SolidError::nurbs(OP, NurbsFailure::Storage));
        }
        if conds.iter().any(|c| !c.t.is_finite() || c.t < a || c.t > b) {
            return refuse(NurbsFailure::Parameter);
        }
        if conds.len() == 1 {
            let c = conds
                .first()
                .copied()
                .ok_or_else(|| SolidError::nurbs(OP, NurbsFailure::Input))?;
            // A constant: the value and a zero slope (the caller supplies zero data for it).
            conds.push(Cond { t: c.t, der: 1 });
        }
        let k = conds.len();
        if k <= 4 {
            return Interpolant::new(Space::polynomial(k.saturating_sub(1).max(1), a, b)?, conds);
        }
        let mut tau: Vec<f64> = conds.iter().map(|c| c.t).collect();
        tau.sort_by(f64::total_cmp);
        let mut breaks = vec![a];
        for j in 1..k - 3 {
            breaks.push(
                (tau.get(j)
                    .copied()
                    .ok_or_else(|| SolidError::nurbs(OP, NurbsFailure::Parameter))?
                    + tau
                        .get(j + 1)
                        .copied()
                        .ok_or_else(|| SolidError::nurbs(OP, NurbsFailure::Parameter))?
                    + tau
                        .get(j + 2)
                        .copied()
                        .ok_or_else(|| SolidError::nurbs(OP, NurbsFailure::Parameter))?)
                    / 3.0,
            );
        }
        breaks.push(b);
        if breaks.windows(2).any(|w| !matches!(w,[a,b] if a < b)) {
            return refuse(NurbsFailure::Parameter);
        }
        Interpolant::new(Space::with_breaks(3, &breaks)?, conds)
    }

    /// Space retained by the factored interpolation operator.
    pub fn space(&self) -> &Space {
        &self.space
    }

    /// Ordered interpolation conditions.
    pub fn conds(&self) -> &[Cond] {
        &self.conds
    }

    /// The control values of the spline meeting the conditions with `data` (one per condition).
    pub fn fit<T: Linear>(&self, data: &[T]) -> Result<Vec<T>> {
        if data.len() != self.conds.len() {
            return refuse(NurbsFailure::CurveDefinition);
        }
        let out = self.lu.solve(data)?;
        if out.iter().any(|x| !x.finite()) {
            return refuse(NurbsFailure::Numerical);
        }
        Ok(out)
    }
}

/// Ordered dense LU with partial pivoting; ties retain the first row.
#[derive(Clone, Debug)]
struct Lu {
    n: usize,
    a: Vec<f64>,
    piv: Vec<usize>,
}
impl Lu {
    fn new(n: usize, mut a: Vec<f64>) -> Result<Self> {
        if n > MAX_SPACE_DIMENSION {
            return Err(SolidError::nurbs(OP, NurbsFailure::Storage));
        }
        if n == 0 || n.checked_mul(n) != Some(a.len()) {
            return refuse(NurbsFailure::CurveDefinition);
        }
        if a.iter().any(|x| !x.is_finite()) {
            return refuse(NurbsFailure::Numerical);
        }
        let scale = a.iter().fold(0.0_f64, |m, x| m.max(x.abs()));
        if scale <= 0.0 || !scale.is_finite() {
            return refuse(NurbsFailure::Singular);
        }
        let mut piv = filled(n, 0usize)?;
        for k in 0..n {
            let (mut best, mut big) = (k, 0.0);
            for r in k..n {
                let x = a
                    .get(r * n + k)
                    .copied()
                    .ok_or_else(|| SolidError::nurbs(OP, NurbsFailure::Index))?
                    .abs();
                if x > big {
                    (best, big) = (r, x);
                }
            }
            if !big.is_finite() || big <= scale * 1e-13 {
                return refuse(NurbsFailure::Singular);
            }
            *piv.get_mut(k)
                .ok_or_else(|| SolidError::nurbs(OP, NurbsFailure::Index))? = best;
            if best != k {
                for c in 0..n {
                    checked_swap(&mut a, k * n + c, best * n + c)?;
                }
            }
            let d = *a
                .get(k * n + k)
                .ok_or_else(|| SolidError::nurbs(OP, NurbsFailure::Index))?;
            for r in k + 1..n {
                let f = *a
                    .get(r * n + k)
                    .ok_or_else(|| SolidError::nurbs(OP, NurbsFailure::Index))?
                    / d;
                *a.get_mut(r * n + k)
                    .ok_or_else(|| SolidError::nurbs(OP, NurbsFailure::Index))? = f;
                if f != 0.0 {
                    for c in k + 1..n {
                        let upper = *a
                            .get(k * n + c)
                            .ok_or_else(|| SolidError::nurbs(OP, NurbsFailure::Index))?;
                        *a.get_mut(r * n + c)
                            .ok_or_else(|| SolidError::nurbs(OP, NurbsFailure::Index))? -=
                            f * upper;
                    }
                }
            }
        }
        Ok(Self { n, a, piv })
    }
    fn solve<T: Linear>(&self, b: &[T]) -> Result<Vec<T>> {
        let n = self.n;
        if b.len() != n {
            return refuse(NurbsFailure::CurveDefinition);
        }
        if b.iter().any(|x| !x.finite()) {
            return refuse(NurbsFailure::Numerical);
        }
        let mut x = filled(n, T::zero())?;
        for (out, value) in x.iter_mut().zip(b) {
            *out = *value;
        }
        for (k, &p) in self.piv.iter().enumerate() {
            checked_swap(&mut x, k, p)?;
        }
        for r in 0..n {
            let row = self
                .a
                .get(r * n..r * n + r)
                .ok_or_else(|| SolidError::nurbs(OP, NurbsFailure::Index))?;
            let prefix = x
                .get(..r)
                .ok_or_else(|| SolidError::nurbs(OP, NurbsFailure::Index))?;
            let initial = *x
                .get(r)
                .ok_or_else(|| SolidError::nurbs(OP, NurbsFailure::Index))?;
            let acc = row
                .iter()
                .zip(prefix)
                .fold(initial, |acc, (l, xc)| acc.plus(xc.times(-l)));
            *x.get_mut(r)
                .ok_or_else(|| SolidError::nurbs(OP, NurbsFailure::Index))? = acc;
        }
        for r in (0..n).rev() {
            let row = self
                .a
                .get(r * n + r + 1..(r + 1) * n)
                .ok_or_else(|| SolidError::nurbs(OP, NurbsFailure::Index))?;
            let suffix = x
                .get(r + 1..)
                .ok_or_else(|| SolidError::nurbs(OP, NurbsFailure::Index))?;
            let initial = *x
                .get(r)
                .ok_or_else(|| SolidError::nurbs(OP, NurbsFailure::Index))?;
            let acc = row
                .iter()
                .zip(suffix)
                .fold(initial, |acc, (u, xc)| acc.plus(xc.times(-u)));
            let diagonal = *self
                .a
                .get(r * n + r)
                .ok_or_else(|| SolidError::nurbs(OP, NurbsFailure::Index))?;
            *x.get_mut(r)
                .ok_or_else(|| SolidError::nurbs(OP, NurbsFailure::Index))? =
                acc.times(1.0 / diagonal);
        }
        Ok(x)
    }
}

/// The control values in `to` of the spline with control values `ctrl` in `from`, which `to`
/// contains: the same domain, a degree at least as high, and every knot of `from` among those of
/// `to` (with a polynomial `from` when the degree rises). Exact: Bézier degree elevation (NURBS
/// Book §5.5) to the target degree, then Boehm knot insertion (§5.2) of the knots `to` adds.
pub fn embed<T: Linear>(from: &Space, ctrl: &[T], to: &Space) -> Result<Vec<T>> {
    if ctrl.len() != from.count() {
        return refuse(NurbsFailure::CurveDefinition);
    }
    let ([a0, b0], [a1, b1]) = (from.domain(), to.domain());
    if a0 != a1 || b0 != b1 {
        return refuse(NurbsFailure::Knots);
    }
    let mut p = from.degree();
    let mut knots = from.knots().to_vec();
    let mut pts: Vec<T> = ctrl.to_vec();
    if p > to.degree() {
        return refuse(NurbsFailure::Knots);
    }
    if p < to.degree() {
        if from.count() != p + 1 {
            return Err(SolidError::nurbs(OP, NurbsFailure::Degree));
        }
        while p < to.degree() {
            pts = elevate_bezier(&pts);
            p += 1;
        }
        knots = filled(p + 1, a0)?;
        knots.extend(std::iter::repeat_n(b0, p + 1));
    }
    // The knots `to` adds: a merge of the two sorted vectors.
    let target = to.knots();
    let mut missing = Vec::new();
    let mut j = 0;
    for &x in target {
        match knots.get(j) {
            Some(&k) if k == x => j += 1,
            Some(&k) if k < x => return refuse(NurbsFailure::Knots),
            _ => missing.push(x),
        }
    }
    if j != knots.len() {
        return refuse(NurbsFailure::Knots);
    }
    let mut h: Vec<[f64; 4]> = pts.iter().map(|x| x.to_h()).collect();
    for x in missing {
        (knots, h) = crate::eval::pcurve::insert_guide_knot(p, &knots, &h, x)
            .map_err(|_| SolidError::nurbs(OP, NurbsFailure::Knots))?;
    }
    if knots.as_slice() != target {
        return refuse(NurbsFailure::Knots);
    }
    if h.iter().flatten().any(|x| !x.is_finite()) {
        return refuse(NurbsFailure::Numerical);
    }
    Ok(h.into_iter().map(T::from_h).collect())
}

/// A Bézier piece's control values one degree higher, the same polynomial (NURBS Book eq. 5.36):
/// `Qᵢ = (i/(p+1))·Pᵢ₋₁ + (1 − i/(p+1))·Pᵢ`.
fn elevate_bezier<T: Linear>(pts: &[T]) -> Vec<T> {
    let p = pts.len().saturating_sub(1);
    let at = |i: usize| pts.get(i).copied().unwrap_or_else(T::zero);
    (0..=p + 1)
        .map(|i| {
            let a = i as f64 / (p + 1) as f64;
            let prev = if i == 0 {
                T::zero()
            } else {
                at(i - 1).times(a)
            };
            prev.plus(at(i).times(1.0 - a))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn number(x: f64) -> [f64; 4] {
        [x, 2.0 * x, -x, 1.0]
    }

    #[test]
    fn confluent_cubic_fit_matches_independently_derived_hermite_controls() {
        // f(t)=2+3t-4t²+5t³; f(0)=2,f'(0)=3,f(1)=6,f'(1)=10.
        let conditions = vec![
            Cond { t: 1., der: 1 },
            Cond { t: 0., der: 0 },
            Cond { t: 1., der: 0 },
            Cond { t: 0., der: 1 },
        ];
        let operator = Interpolant::new(Space::polynomial(3, 0., 1.).unwrap(), conditions).unwrap();
        let controls = operator
            .fit(&[number(10.), number(2.), number(6.), number(3.)])
            .unwrap();
        for (actual, want) in controls.iter().zip([2., 3., 8. / 3., 6.]) {
            assert!((actual[0] - want).abs() < 1e-12);
        }
        for t in [0., 0.125, 0.5, 0.875, 1.] {
            let p = operator.space().eval(&controls, t, 0).unwrap();
            let expected = 2. + 3. * t - 4. * t * t + 5. * t * t * t;
            assert!((p[0] - expected).abs() < 1e-12);
            assert!((p[1] - 2. * expected).abs() < 1e-12);
            assert!((p[2] + expected).abs() < 1e-12);
        }
    }

    #[test]
    fn ordered_lu_preserves_first_maximum_pivot_and_known_solution() {
        // A*[1,2,3]=[5,6,8]; the first-column maximum ties in rows1 and2.
        let factor = Lu::new(3, vec![0., 1., 1., 2., 2., 0., 2., 0., 2.]).unwrap();
        assert_eq!(factor.piv[0], 1);
        let result = factor.solve(&[[5.; 4], [6.; 4], [8.; 4]]).unwrap();
        assert_eq!(result, [[1.; 4], [2.; 4], [3.; 4]]);
        assert!(Lu::new(2, vec![1., 1., 1., 1.]).is_err());
        assert!(factor.solve(&[[1.; 4]]).is_err());
        assert!(factor.solve(&[[f64::NAN; 4]; 3]).is_err());
    }

    #[test]
    fn embedding_preserves_homogeneous_values_and_direct_difference_factors() {
        let from = Space::polynomial(1, 0., 1.).unwrap();
        let to = Space::with_breaks(3, &[0., 0.125, 0.5, 1.]).unwrap();
        let values = [[2., 3., 4., 1.], [5., -2., 8., 2.]];
        let embedded = embed(&from, &values, &to).unwrap();
        for t in [0., 0.01, 0.125, 0.5, 0.99, 1.] {
            let before = from.eval(&values, t, 0).unwrap();
            let after = to.eval(&embedded, t, 0).unwrap();
            for (a, b) in before.into_iter().zip(after) {
                assert!((a - b).abs() < 1e-12);
            }
        }
        assert!(embed(&to, &embedded, &from).is_err());
        let from = Space::polynomial(1, -1., 2.).unwrap();
        let tiny = 2.0 - f64::EPSILON;
        let target = Space::with_breaks(1, &[-1., tiny, 2.]).unwrap();
        let source = [[1., 0., 0., 1e12], [2., 0., 0., 1.]];
        let split = embed(&from, &source, &target).unwrap();
        let alpha = (tiny + 1.) / 3.;
        let beta = (2. - tiny) / 3.;
        let want = source[1][3] * alpha + source[0][3] * beta;
        assert_eq!(split[1][3], want);
        assert_ne!(
            split[1][3],
            source[1][3] * alpha + source[0][3] * (1. - alpha)
        );
    }

    #[test]
    fn hostile_space_and_interpolation_requests_are_bounded_refusals() {
        assert!(Space::polynomial(usize::MAX, 0., 1.).is_err());
        assert!(Space::with_breaks(usize::MAX, &[0., 1.]).is_err());
        assert!(Space::new(1, vec![0., 0., 0.5, 0.5, 1., 1.]).is_err());
        assert!(Space::new(1, vec![0., 0., f64::NAN, 1.]).is_err());
        let space = Space::polynomial(3, 0., 1.).unwrap();
        assert!(space.basis(f64::NAN, 0).is_err());
        assert!(space
            .basis(0.5, usize::MAX)
            .unwrap()
            .1
            .iter()
            .all(|&x| x == 0.));
        assert!(Interpolant::new(space.clone(), vec![Cond { t: 0.5, der: 0 }; 4]).is_err());
        assert!(Interpolant::lowest(
            vec![Cond { t: 0.5, der: 0 }; MAX_SPACE_DIMENSION + 1],
            0.,
            1.
        )
        .is_err());
        assert!(space.refined(&vec![0.5; MAX_SPACE_DIMENSION + 1]).is_err());
    }

    #[test]
    fn standard_nurbs_failures_keep_unsupported_limit_and_invalid_classification() {
        let unsupported = Space::polynomial(usize::MAX, 0., 1.).unwrap_err();
        assert_eq!(unsupported.nurbs_failure(), Some(&NurbsFailure::Degree));
        let limit = Space::with_breaks(1, &vec![0.; MAX_SPACE_DIMENSION + 1]).unwrap_err();
        assert_eq!(limit.nurbs_failure(), Some(&NurbsFailure::Storage));
        let invalid = Space::new(1, vec![0., 0., f64::NAN, 1.]).unwrap_err();
        assert_eq!(invalid.nurbs_failure(), Some(&NurbsFailure::Knots));
        let singular = Interpolant::new(
            Space::polynomial(1, 0., 1.).unwrap(),
            vec![Cond { t: 0.5, der: 0 }; 2],
        )
        .unwrap_err();
        assert_eq!(singular.nurbs_failure(), Some(&NurbsFailure::Singular));
        for error in [unsupported, limit, invalid, singular] {
            assert!(!error.is_defect());
        }
    }
    #[test]
    fn ordered_fill_band_and_dense_solve_the_analytic_tridiagonal_system() {
        use crate::geom::ordered_solve::{Band, Normal};
        // A has diagonal 4 and adjacent entries -1. x_i=i+1 gives
        // b=(2,4,...,14,25) for eight scalar coordinates.
        let rhs = vec![2., 4., 6., 8., 10., 12., 14., 25.];
        let mut left = Band::new(8, 1).unwrap();
        let mut right = Normal::new(8, 1).unwrap();
        let mut dense = Normal::new(8, 7).unwrap();
        for r in 0usize..8 {
            for s in 0usize..8 {
                let value = if r == s {
                    4.
                } else if r.abs_diff(s) == 1 {
                    -1.
                } else {
                    0.
                };
                left.add(r, s, value);
                right.add(r, s, value);
                dense.add(r, s, value);
            }
        }
        for actual in [
            left.solve(rhs.clone()).unwrap(),
            right.solve(rhs.clone()).unwrap(),
            dense.solve(rhs).unwrap(),
        ] {
            for (i, value) in actual.into_iter().enumerate() {
                assert!((value - (i + 1) as f64).abs() < 1e-12);
            }
        }
    }

    #[test]
    fn ordered_fill_schur_elimination_and_cross_patch_promotion_preserve_known_solution() {
        use crate::geom::ordered_solve::Normal;
        // The two interiors meet at shared coordinate 2. Optionally coupling
        // coordinates 1 and 3 promotes both into the dense Schur block.
        for cross in [false, true] {
            let mut matrix =
                Normal::blocks(vec![Some(0), Some(0), None, Some(1), Some(1)]).unwrap();
            for r in 0usize..5 {
                for s in 0usize..5 {
                    let value = if r == s {
                        4.
                    } else if r.abs_diff(s) == 1 {
                        -1.
                    } else if cross && matches!((r, s), (1, 3) | (3, 1)) {
                        0.25
                    } else {
                        0.
                    };
                    matrix.add(r, s, value);
                }
            }
            let rhs = if cross {
                vec![6., -8.75, 1., 12., -7.]
            } else {
                vec![6., -9.5, 1., 12.5, -7.]
            };
            let actual = matrix.solve(rhs).unwrap();
            for (value, want) in actual.into_iter().zip([1., -2., 0.5, 3., -1.]) {
                assert!((value - want).abs() < 1e-12);
            }
        }
        let mut no_shared = Normal::blocks(vec![Some(0), Some(0)]).unwrap();
        no_shared.add(0, 0, 2.);
        no_shared.add(1, 1, 4.);
        for (value, want) in no_shared
            .solve(vec![6., -8.])
            .unwrap()
            .into_iter()
            .zip([3., -2.])
        {
            assert!((value - want).abs() < 1e-12);
        }
    }

    #[test]
    fn ordered_fill_refuses_hostile_dimensions_shapes_and_singular_systems_with_standard_errors() {
        use crate::error::nurbs::LinearSolveFailure as F;
        use crate::geom::ordered_solve::{Band, Normal, MAX_SHARED_UNKNOWNS};
        let check = |error: SolidError, reason: F| {
            assert_eq!(
                error.nurbs_failure(),
                Some(&NurbsFailure::LinearSolve(reason))
            );
            assert!(!error.is_defect());
        };
        check(Band::new(usize::MAX, 0).err().unwrap(), F::TooLarge);
        check(Band::new(1, usize::MAX).err().unwrap(), F::InvalidShape);
        check(
            Normal::blocks(vec![Some(usize::MAX)]).err().unwrap(),
            F::InvalidShape,
        );
        check(
            Band::new(1, 0).unwrap().solve(vec![0.]).unwrap_err(),
            F::FitSingular,
        );
        check(
            Normal::new(2, 1).unwrap().solve(vec![0., 0.]).unwrap_err(),
            F::PatchSingular,
        );
        let mut band = Band::new(1, 0).unwrap();
        band.add(0, 0, 1.);
        check(band.clone().solve(vec![]).unwrap_err(), F::InvalidShape);
        check(band.solve(vec![f64::NAN]).unwrap_err(), F::FitNonFinite);
        let mut dense = Normal::new(1, 1).unwrap();
        dense.add(0, 0, 1.);
        check(dense.solve(vec![f64::NAN]).unwrap_err(), F::PatchNonFinite);
        let mut blocks = Normal::blocks(vec![None]).unwrap();
        blocks.add(usize::MAX, 0, 1.);
        check(blocks.solve(vec![0.]).unwrap_err(), F::InvalidShape);
        let count = MAX_SHARED_UNKNOWNS + 1;
        check(
            Normal::blocks(vec![None; count])
                .unwrap()
                .solve(vec![0.; count])
                .unwrap_err(),
            F::SharedLimit { given: count },
        );
        let mut band = Band::new(1, 0).unwrap();
        band.add(0, 0, 4.);
        band.add(usize::MAX, usize::MAX, 1.);
        assert_eq!(band.solve(vec![4.]).unwrap(), vec![1.]);
    }
}
