//! Geometric continuity of order `n` (G0 … G4) between two surfaces that share a curve, in the
//! linkage-curve formulation, as the fill imposes it and as a seam is measured.
//!
//! **The criterion.** Two surfaces meeting along a curve are G^n there exactly when the
//! *linkage curves* crossing it — a curve on one surface continued by a curve on the other — are
//! G^n curves (Pegna & Wolter, "Geometrical criteria to guarantee curvature continuity of blend
//! surfaces", J. Mech. Des. 114, 1992, for n = 2; Hermann, Lukács & Wolter, "Geometrical
//! criteria on the higher order smoothness of composite surfaces", CAGD 16, 1999, for any n).
//! In a form that needs no reparameterisation: write the neighbour, near a point `p` of the
//! curve, as a height `H` over its tangent plane along its unit normal `n`,
//! `q = p + x + H(x)·n` for `x` in the tangent plane, so that `H(0) = 0` and `∇H(0) = 0`. A
//! curve `c(σ)` from `p` lies on a surface meeting the neighbour with G^n exactly when its height
//! above the neighbour, `n·(c(σ) − p) − H(x(σ))` with `x(σ)` the part of `c(σ) − p` in the
//! tangent plane, vanishes to order `n` in σ. With `H(x) = Σ_{m ≥ 2} H_m(x) / m!` (`H_m` the
//! m-th derivative of `H` at `p`, a form of degree m) and `x(σ) = Σ_j x_j σʲ / j!`
//! (`x_j` the tangent-plane part of `c⁽ʲ⁾`), the condition of order `k` is
//!
//! `n·c⁽ᵏ⁾ = k! · [σᵏ] Σ_{m=2}^{k} H_m(x(σ)) / m!`,
//!
//! whose right-hand side reads `c′ … c⁽ᵏ⁻¹⁾` only: order 1 is `n·c′ = 0` (the tangent plane),
//! order 2 `n·c″ = H₂(x₁)` (the normal curvature in the curve's direction), order 3
//! `n·c‴ = 3·H₂(x₁, x₂) + H₃(x₁)` (the rate of change of that curvature), and order 4 its rate of
//! change in turn. For a unit `a`, `H₂(a)` is the curvature of the normal section along `a` and
//! `H₃(a)`, `H₄(a)` are its first and second derivatives by arc length.
//!
//! **The jet.** [`surface_jet`] reads `H₂ … H_n` off a surface record's exact partial
//! derivatives: along a tangent vector `a`, the curve on the surface whose foot in the tangent
//! plane runs straight along `a` at unit rate has `n·γ⁽ᵐ⁾(0) = H_m(a)`; its parameter series
//! `(u(τ), v(τ))` is found order by order (the part of each coefficient of `S(u(τ), v(τ))` in the
//! tangent plane is `a`, then zero), and `H_m` as a binary form from its values along
//! `t + λ·w` at `m + 1` values of λ (a polynomial of degree m in λ, so exactly).
//!
//! Input is hostile: orders are capped at [`MAX_CONTINUITY`], and every evaluation is checked.

use crate::Vec3;
use crate::brep::eval::SurfaceEval;
use crate::tess::partials::{MAX_PARTIAL_ORDER, Partials};

/// The highest continuity order handled: G4, the highest order of the surfaces' exact partials.
pub const MAX_CONTINUITY: usize = MAX_PARTIAL_ORDER;

/// A surface near a point as a height over its tangent plane, to order `order` (2 … 4): the
/// forms `H_m` for `m = 2 ..= order` on the frame `t = n × w`, `w` (both unit, in the tangent
/// plane; `w` the direction the surface leaves the shared curve in). `forms[m − 2][j]` is the
/// coefficient of `x_t^(m−j)·x_w^j` in `H_m(x)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Jet {
    pub order: usize,
    pub forms: [[f64; MAX_CONTINUITY + 1]; MAX_CONTINUITY - 1],
}

impl Jet {
    /// `H_m(a)` for a tangent vector given by its components `(a_t, a_w)`.
    pub fn form(&self, m: usize, a: [f64; 2]) -> Option<f64> {
        let coefficients = self.forms.get(m.checked_sub(2)?).filter(|_| (2..=self.order).contains(&m))?;
        Some((0..=m).map(|j| coefficients.get(j).copied().unwrap_or(0.0) * a[0].powi((m - j) as i32) * a[1].powi(j as i32)).sum())
    }

    /// Whether every coefficient is finite.
    pub fn is_finite(&self) -> bool {
        self.forms.iter().flatten().all(|x| x.is_finite())
    }

    /// What orders `2 ..= self.order` ask of `n·c⁽ᵏ⁾` (module docs), for a curve whose first
    /// derivatives have the tangent-plane components `x[j − 1] = (x_j·t, x_j·w)`: element
    /// `k − 2` is order k's (it reads `x[0 .. k − 1]`). `None` when too few derivatives are given.
    pub fn targets(&self, x: &[[f64; 2]]) -> Option<Vec<f64>> {
        Some(self.linearised(x)?.into_iter().map(|t| t.value).collect())
    }

    /// [`Jet::targets`] with their exact gradients: element `k − 2` is order k's target and its
    /// derivative with respect to each component of each `x_j` (`gradient[j − 1]`), so that the
    /// condition `n·c⁽ᵏ⁾ = target(x)` can be linearised about a curve (Newton's method).
    pub fn linearised(&self, x: &[[f64; 2]]) -> Option<Vec<Target>> {
        let top = self.order;
        if x.len() + 1 < top || top > MAX_CONTINUITY {
            return None;
        }
        // x(σ) as truncated Taylor series in each component (no constant term), each coefficient
        // a dual number seeded with its own component.
        let mut series = [vec![Dual::ZERO; top + 1], vec![Dual::ZERO; top + 1]];
        for (j, xj) in x.iter().enumerate().take(top.saturating_sub(1)) {
            for (c, (s, value)) in series.iter_mut().zip(xj).enumerate() {
                if let Some(slot) = s.get_mut(j + 1) {
                    *slot = Dual::seed(*value, 2 * j + c).scale(1.0 / factorial(j + 1));
                }
            }
        }
        let mut height = vec![Dual::ZERO; top + 1];
        for m in 2..=top {
            let coefficients = self.forms.get(m - 2)?;
            for (j, c) in coefficients.iter().enumerate().take(m + 1) {
                let monomial = mul(&power(&series[0], m - j, top), &power(&series[1], j, top), top);
                for (h, x) in height.iter_mut().zip(&monomial) {
                    *h = *h + x.scale(c / factorial(m));
                }
            }
        }
        Some(
            (2..=top)
                .map(|k| {
                    let d = height.get(k).copied().unwrap_or(Dual::ZERO).scale(factorial(k));
                    let mut gradient = [[0.0; 2]; MAX_CONTINUITY - 1];
                    for (j, g) in gradient.iter_mut().enumerate() {
                        *g = [d.grad[2 * j], d.grad[2 * j + 1]];
                    }
                    Target { value: d.value, gradient }
                })
                .collect(),
        )
    }
}

/// One order's target and its gradient (see [`Jet::linearised`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Target {
    pub value: f64,
    /// `gradient[j − 1]`: the derivative with respect to `(x_j·t, x_j·w)`.
    pub gradient: [[f64; 2]; MAX_CONTINUITY - 1],
}

/// Gradients carried: two components of each of `x₁ … x₃`.
const SEEDS: usize = 2 * (MAX_CONTINUITY - 1);

/// A number with its gradient with respect to the [`SEEDS`] components (forward differentiation:
/// the targets are polynomials in them, so the gradient is exact).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Dual {
    value: f64,
    grad: [f64; SEEDS],
}

impl Dual {
    const ZERO: Dual = Dual { value: 0.0, grad: [0.0; SEEDS] };
    const ONE: Dual = Dual { value: 1.0, grad: [0.0; SEEDS] };

    fn seed(value: f64, at: usize) -> Dual {
        let mut grad = [0.0; SEEDS];
        if let Some(g) = grad.get_mut(at) {
            *g = 1.0;
        }
        Dual { value, grad }
    }

    fn scale(self, k: f64) -> Dual {
        Dual { value: self.value * k, grad: self.grad.map(|g| g * k) }
    }
}

impl std::ops::Add for Dual {
    type Output = Dual;
    fn add(self, o: Dual) -> Dual {
        let mut grad = self.grad;
        for (g, h) in grad.iter_mut().zip(o.grad) {
            *g += h;
        }
        Dual { value: self.value + o.value, grad }
    }
}

impl std::ops::Mul for Dual {
    type Output = Dual;
    fn mul(self, o: Dual) -> Dual {
        let mut grad = [0.0; SEEDS];
        for ((g, a), b) in grad.iter_mut().zip(self.grad).zip(o.grad) {
            *g = a * o.value + self.value * b;
        }
        Dual { value: self.value * o.value, grad }
    }
}

/// `k!` as a float (small k).
fn factorial(k: usize) -> f64 {
    (1..=k).map(|i| i as f64).product()
}

/// The numbers the series below are over: plain floats and [`Dual`]s.
trait Coefficient: Copy + std::ops::Add<Output = Self> + std::ops::Mul<Output = Self> {
    const ZERO: Self;
    const ONE: Self;
}

impl Coefficient for f64 {
    const ZERO: f64 = 0.0;
    const ONE: f64 = 1.0;
}

impl Coefficient for Dual {
    const ZERO: Dual = Dual::ZERO;
    const ONE: Dual = Dual::ONE;
}

/// The product of two truncated series (coefficients of `σ⁰ … σ^top`).
fn mul<T: Coefficient>(a: &[T], b: &[T], top: usize) -> Vec<T> {
    let mut out = vec![T::ZERO; top + 1];
    for (i, x) in a.iter().enumerate().take(top + 1) {
        for (j, y) in b.iter().enumerate().take(top + 1 - i) {
            if let Some(slot) = out.get_mut(i + j) {
                *slot = *slot + *x * *y;
            }
        }
    }
    out
}

/// A truncated series to the power `e`.
fn power<T: Coefficient>(a: &[T], e: usize, top: usize) -> Vec<T> {
    let mut one = vec![T::ZERO; top + 1];
    if let Some(c) = one.first_mut() {
        *c = T::ONE;
    }
    (0..e).fold(one, |acc, _| mul(&acc, a, top))
}

/// The surface `s` near `uv` as a height along its unit normal `n` (either sign) over its tangent
/// plane, to order `order` (2 ..= [`MAX_CONTINUITY`]), on the frame `t = n × w`, `w` (`w` a unit
/// tangent vector): see [`Jet`] and the module docs. `None` where the surface has no tangent
/// plane (a pole) or cannot be evaluated.
pub fn surface_jet(s: &SurfaceEval, uv: [f64; 2], n: Vec3, w: Vec3, order: usize) -> Option<Jet> {
    if !(2..=MAX_CONTINUITY).contains(&order) {
        return None;
    }
    let d = s.partials(uv[0], uv[1])?;
    let t = n.cross(w);
    let mut forms = [[0.0; MAX_CONTINUITY + 1]; MAX_CONTINUITY - 1];
    // H_m(t + λ w) is a polynomial of degree m in λ: its coefficients are the form's.
    const NODES: [f64; MAX_CONTINUITY + 1] = [0.0, 1.0, -1.0, 2.0, -2.0];
    let along: Vec<Vec<f64>> = NODES.iter().map(|l| heights(&d, n, t + w * *l, order)).collect::<Option<_>>()?;
    for m in 2..=order {
        let values: Vec<f64> = along.iter().take(m + 1).map(|h| h.get(m).copied().unwrap_or(f64::NAN)).collect();
        let coefficients = polynomial_through(NODES.get(..=m)?, &values)?;
        let slot = forms.get_mut(m - 2)?;
        for (x, c) in slot.iter_mut().zip(coefficients) {
            *x = c;
        }
    }
    let jet = Jet { order, forms };
    jet.is_finite().then_some(jet)
}

/// `H_m(a)` for `m = 0 ..= order` from the partials `d` (`d[i][j] = S^(i,j)`): the curve on
/// the surface whose foot runs straight along `a` (module docs), its parameter series solved
/// order by order. `None` where the tangent plane degenerates.
fn heights(d: &Partials, n: Vec3, a: Vec3, order: usize) -> Option<Vec<f64>> {
    let at = |i: usize, j: usize| d.get(i).and_then(|r| r.get(j)).copied();
    let (su, sv) = (at(1, 0)?, at(0, 1)?);
    let (e, f, g) = (su.dot(su), su.dot(sv), sv.dot(sv));
    let det = e * g - f * f;
    // A pole: one parameter direction collapses against the other (or the two run together).
    if !(e.min(g) > 1e-20 * e.max(g) && det > 1e-12 * e * g) {
        return None;
    }
    // The rates (du, dv) whose image S_u du + S_v dv has tangent-plane part `b`.
    let rates = |b: Vec3| {
        let (bu, bv) = (b.dot(su), b.dot(sv));
        ((g * bu - f * bv) / det, (e * bv - f * bu) / det)
    };
    let tangential = |x: Vec3| x - n * x.dot(n);
    let mut du = vec![0.0; order + 1];
    let mut dv = vec![0.0; order + 1];
    for k in 1..=order {
        // The coefficient of τᵏ with this order's rates still nought, then the rates that make
        // its tangent-plane part `a` (k = 1) or zero.
        let have = composed(d, &du, &dv, order)?.get(k).copied()?;
        let want = if k == 1 { tangential(a) } else { Vec3::ZERO };
        let (x, y) = rates(want - tangential(have));
        *du.get_mut(k)? = x;
        *dv.get_mut(k)? = y;
    }
    let c = composed(d, &du, &dv, order)?;
    Some(c.iter().enumerate().map(|(k, x)| factorial(k) * n.dot(*x)).collect())
}

/// The Taylor coefficients of `S(u₀ + δu(τ), v₀ + δv(τ)) − S(u₀, v₀)` to order `order`, for the
/// series `δu`, `δv` (no constant term) and the partials `d`.
fn composed(d: &Partials, du: &[f64], dv: &[f64], order: usize) -> Option<Vec<Vec3>> {
    let mut out = vec![Vec3::ZERO; order + 1];
    for (i, row) in d.iter().enumerate() {
        for (j, partial) in row.iter().enumerate() {
            if i + j == 0 || i + j > order {
                continue;
            }
            let term = mul(&power(du, i, order), &power(dv, j, order), order);
            let scale = 1.0 / (factorial(i) * factorial(j));
            for (o, x) in out.iter_mut().zip(&term) {
                *o += *partial * (x * scale);
            }
        }
    }
    out.iter().all(|p| p.is_finite()).then_some(out)
}

/// The coefficients `c₀ … c_m` of the polynomial of degree `m = nodes.len() − 1` through
/// `(nodes[i], values[i])` (Newton's divided differences, expanded).
fn polynomial_through(nodes: &[f64], values: &[f64]) -> Option<Vec<f64>> {
    let m = nodes.len();
    if values.len() != m || m == 0 {
        return None;
    }
    let mut dd = values.to_vec();
    for level in 1..m {
        for i in (level..m).rev() {
            let span = nodes.get(i)? - nodes.get(i - level)?;
            if span == 0.0 {
                return None;
            }
            *dd.get_mut(i)? = (dd.get(i)? - dd.get(i - 1)?) / span;
        }
    }
    // Horner on the Newton form: p(λ) = dd₀ + (λ − x₀)(dd₁ + (λ − x₁)(…)).
    let mut c = vec![0.0; m];
    for i in (0..m).rev() {
        // c ← c·(λ − x_i) + dd_i.
        let x = *nodes.get(i)?;
        let mut next = vec![0.0; m];
        for (k, ck) in c.iter().enumerate() {
            if let Some(slot) = next.get_mut(k + 1) {
                *slot += ck;
            }
            if let Some(slot) = next.get_mut(k) {
                *slot -= ck * x;
            }
        }
        if let Some(slot) = next.get_mut(0) {
            *slot += dd.get(i)?;
        }
        c = next;
    }
    c.iter().all(|x| x.is_finite()).then_some(c)
}

#[cfg(test)]
#[path = "linkage_tests.rs"]
mod tests;
