//! Exact partial derivatives of the display evaluator's surfaces up to the fourth order, and the
//! derivatives of a rational spline curve: what a seam's change of curvature (G3) and the change
//! of that change (G4) are read from. Splines differentiate their basis functions
//! ([`crate::bspline::basis_derivatives`]) and divide out the weight by Leibniz's rule (NURBS
//! Book eq. 4.8 and 4.20); a revolution turns its profile's derivatives by the rotation's own.

use super::{EvalCurve, EvalSurface, affine, linear, onto_knots};
use crate::Vec3;
use crate::bspline::{MAX_SPLINE_DEGREE, basis_derivatives, binomial};

/// The highest total order of [`Partials`].
pub const MAX_PARTIAL_ORDER: usize = 4;

/// A surface's partial derivatives at a point: `d[i][j]` is `∂^(i+j) S / ∂u^i ∂v^j`, filled for
/// `i + j ≤` [`MAX_PARTIAL_ORDER`] (`d[0][0]` the point itself) and zero past it.
pub type Partials = [[Vec3; MAX_PARTIAL_ORDER + 1]; MAX_PARTIAL_ORDER + 1];

/// The pairs `(i, j)` with `i + j ≤ MAX_PARTIAL_ORDER`.
pub(crate) fn orders() -> impl Iterator<Item = (usize, usize)> {
    (0..=MAX_PARTIAL_ORDER).flat_map(|i| (0..=MAX_PARTIAL_ORDER - i).map(move |j| (i, j)))
}

/// A table of partials from `f(i, j)`; `None` when one is missing or not finite.
pub(crate) fn tabulate(mut f: impl FnMut(usize, usize) -> Option<Vec3>) -> Option<Partials> {
    let mut d = [[Vec3::ZERO; MAX_PARTIAL_ORDER + 1]; MAX_PARTIAL_ORDER + 1];
    for (i, j) in orders() {
        let x = f(i, j).filter(|x| x.is_finite())?;
        *d.get_mut(i)?.get_mut(j)? = x;
    }
    Some(d)
}

impl EvalSurface {
    /// The partial derivatives at (u, v) up to the fourth order ([`Partials`]), exactly: a closed
    /// spline's parameters wrapped first as in [`EvalSurface::derivs`]. `None` where the surface
    /// cannot be evaluated (a malformed spline, a vanishing weight).
    pub fn partials(&self, u: f64, v: f64) -> Option<Partials> {
        if !(u.is_finite() && v.is_finite()) {
            return None;
        }
        match self {
            EvalSurface::Plane { origin, du, dv } => tabulate(|i, j| {
                Some(match (i, j) {
                    (0, 0) => *origin + *du * u + *dv * v,
                    (1, 0) => *du,
                    (0, 1) => *dv,
                    _ => Vec3::ZERO,
                })
            }),
            EvalSurface::Nurbs { degree, knots, count, ctrl, .. } => {
                let [u, v] = self.wrap([u, v]);
                nurbs_partials(*degree, knots, *count, ctrl, u, v)
            }
            EvalSurface::Revolved { profile, origin, axis, transform, swap } => {
                let [u, v] = self.wrap([u, v]);
                let (s, a) = if *swap { (v, u) } else { (u, v) };
                let c = profile.derivatives(s, MAX_PARTIAL_ORDER)?;
                let turned = |i: usize, j: usize| -> Option<Vec3> {
                    // The profile's i-th derivative (its point taken about the axis), turned by
                    // the j-th derivative of the rotation.
                    let x = if i == 0 { *c.first()? - *origin } else { *c.get(i)? };
                    let r = rotation_derivative(*axis, x, a, j);
                    Some(if i == 0 && j == 0 { affine(transform, *origin + r) } else { linear(transform, r) })
                };
                tabulate(|i, j| if *swap { turned(j, i) } else { turned(i, j) })
            }
        }
    }
}

/// The `j`-th derivative by the angle `a` of `x` turned by `a` about the unit `axis` (Rodrigues:
/// `x cos a + (axis × x) sin a + axis (axis · x)(1 − cos a)`).
fn rotation_derivative(axis: Vec3, x: Vec3, a: f64, j: usize) -> Vec3 {
    let (cos, sin) = trig_derivative(a, j);
    let along = axis * axis.dot(x);
    let constant = if j == 0 { along } else { Vec3::ZERO };
    x * cos + axis.cross(x) * sin - along * cos + constant
}

/// The `k`-th derivatives of cos and sin at `t`: `cos(t + kπ/2)` and `sin(t + kπ/2)`.
pub(crate) fn trig_derivative(t: f64, k: usize) -> (f64, f64) {
    let (sin, cos) = t.sin_cos();
    match k % 4 {
        0 => (cos, sin),
        1 => (-sin, cos),
        2 => (-cos, -sin),
        _ => (sin, -cos),
    }
}

/// The basis functions alive at `t` and their derivatives up to `order`: the index of the first
/// and, for each, element `k` its `k`-th derivative (a `t` past an end by rounding is that end).
fn basis(knots: &[f64], p: usize, n: usize, t: f64, order: usize) -> Option<(usize, Vec<Vec<f64>>)> {
    basis_derivatives(knots, p, n, onto_knots(knots, p, n, t)?, order)
}

fn valid(p: usize, n: usize, knots: &[f64]) -> bool {
    (1..=MAX_SPLINE_DEGREE).contains(&p) && n > p && knots.len() == n + p + 1
}

fn xyz(h: [f64; 4]) -> Vec3 {
    Vec3::new(h[0], h[1], h[2])
}

fn nurbs_partials(degree: [usize; 2], knots: &[Vec<f64>; 2], count: [usize; 2], ctrl: &[[f64; 4]], u: f64, v: f64) -> Option<Partials> {
    let [pu, pv] = degree;
    let [nu, nv] = count;
    if !valid(pu, nu, &knots[0]) || !valid(pv, nv, &knots[1]) || nu.checked_mul(nv)? != ctrl.len() {
        return None;
    }
    let ((fu, bu), (fv, bv)) = (basis(&knots[0], pu, nu, u, MAX_PARTIAL_ORDER)?, basis(&knots[1], pv, nv, v, MAX_PARTIAL_ORDER)?);
    // The homogeneous partials A_ij = Σ N_a^(i) N_b^(j) P_ab (weights in the fourth place).
    let mut a = [[[0.0f64; 4]; MAX_PARTIAL_ORDER + 1]; MAX_PARTIAL_ORDER + 1];
    for (jb, nb) in bv.iter().enumerate() {
        for (ia, na) in bu.iter().enumerate() {
            let c = ctrl.get((fv + jb) * nu + fu + ia)?;
            for (i, j) in orders() {
                let w = na.get(i)? * nb.get(j)?;
                let sum = a.get_mut(i)?.get_mut(j)?;
                for (s, c) in sum.iter_mut().zip(c) {
                    *s += w * c;
                }
            }
        }
    }
    let w = a[0][0][3];
    if !(w.abs() > 1e-300) {
        return None;
    }
    // S_kl = (A_kl − Σ_{(i,j) ≠ (0,0)} C(k,i) C(l,j) w_ij S_(k−i)(l−j)) / w, lower orders first.
    let mut d = [[Vec3::ZERO; MAX_PARTIAL_ORDER + 1]; MAX_PARTIAL_ORDER + 1];
    for total in 0..=MAX_PARTIAL_ORDER {
        for k in 0..=total {
            let l = total - k;
            let mut x = xyz(a[k][l]);
            for (i, j) in (0..=k).flat_map(|i| (0..=l).map(move |j| (i, j))).filter(|ij| *ij != (0, 0)) {
                x = x - d[k - i][l - j] * (binomial(k, i) * binomial(l, j) * a[i][j][3]);
            }
            d[k][l] = x * (1.0 / w);
        }
    }
    tabulate(|i, j| d.get(i)?.get(j).copied())
}

impl EvalCurve {
    /// The point and derivatives up to `order` at `t` (element `k` the `k`-th), exactly: the
    /// weight divided out by Leibniz's rule. `None` if the curve is malformed or its weight
    /// vanishes there.
    pub fn derivatives(&self, t: f64, order: usize) -> Option<Vec<Vec3>> {
        let (p, n) = (self.degree, self.ctrl.len());
        if !valid(p, n, &self.knots) || !t.is_finite() || order > MAX_PARTIAL_ORDER {
            return None;
        }
        let (first, b) = basis(&self.knots, p, n, t, order)?;
        let mut a = vec![[0.0f64; 4]; order + 1];
        for (k, nk) in b.iter().enumerate() {
            let c = self.ctrl.get(first + k)?;
            for (ak, w) in a.iter_mut().zip(nk) {
                for (s, c) in ak.iter_mut().zip(c) {
                    *s += w * c;
                }
            }
        }
        let w = a.first()?[3];
        if !(w.abs() > 1e-300) {
            return None;
        }
        let mut d: Vec<Vec3> = Vec::with_capacity(order + 1);
        for k in 0..=order {
            let mut x = xyz(*a.get(k)?);
            for i in 1..=k {
                x = x - *d.get(k - i)? * (binomial(k, i) * a.get(i)?[3]);
            }
            d.push(x * (1.0 / w));
        }
        d.iter().all(|x| x.is_finite()).then_some(d)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    /// Within `tol`, relative to the larger length (at least 1).
    fn close(a: Vec3, b: Vec3, tol: f64) -> bool {
        a.dist(b) <= tol * (1.0 + a.len().max(b.len()))
    }

    fn quarter_cylinder() -> EvalSurface {
        // A rational quadratic quarter circle of radius 2 in u, a line 3 long in v: exact.
        let w = std::f64::consts::FRAC_1_SQRT_2;
        EvalSurface::nurbs(
            [2, 1],
            [vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0], vec![0.0, 0.0, 1.0, 1.0]],
            [3, 2],
            vec![
                [2.0, 0.0, 0.0, 1.0],
                [2.0 * w, 2.0 * w, 0.0, w],
                [0.0, 2.0, 0.0, 1.0],
                [2.0, 0.0, 3.0, 1.0],
                [2.0 * w, 2.0 * w, 3.0 * w, w],
                [0.0, 2.0, 3.0, 1.0],
            ],
        )
    }

    #[test]
    fn every_partial_is_the_derivative_of_the_order_below() {
        let line = EvalCurve { degree: 1, knots: vec![0.0, 0.0, 1.0, 1.0], ctrl: vec![[2.0, 0.0, 0.0, 1.0], [3.0, 0.0, 1.0, 1.0]] };
        let cubic = EvalSurface::nurbs(
            [3, 2],
            [vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0], vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0]],
            [4, 3],
            (0..12).map(|k| [k as f64 * 0.7, (k * k) as f64 * 0.1, (k % 5) as f64, 1.0 + 0.1 * (k % 3) as f64]).collect(),
        );
        let surfaces = [
            quarter_cylinder(),
            cubic,
            EvalSurface::Revolved {
                profile: line,
                origin: Vec3::ZERO,
                axis: Vec3::Z,
                transform: [[1.0, 0.0, 0.0, 1.0], [0.0, 0.0, -1.0, 2.0], [0.0, 1.0, 0.0, 3.0]],
                swap: true,
            },
        ];
        let h = 1e-4;
        for s in &surfaces {
            let (u, v) = (0.37, 0.61);
            let d = s.partials(u, v).unwrap();
            let (p, su, sv) = s.derivs(u, v).unwrap();
            assert!(close(d[0][0], p, 1e-12) && close(d[1][0], su, 1e-12) && close(d[0][1], sv, 1e-12), "{s:?}");
            let (up, um) = (s.partials(u + h, v).unwrap(), s.partials(u - h, v).unwrap());
            let (vp, vm) = (s.partials(u, v + h).unwrap(), s.partials(u, v - h).unwrap());
            for (i, j) in orders().filter(|(i, j)| i + j < MAX_PARTIAL_ORDER) {
                let du = (up[i][j] - um[i][j]) * (0.5 / h);
                let dv = (vp[i][j] - vm[i][j]) * (0.5 / h);
                assert!(close(du, d[i + 1][j], 1e-5), "d/du of ({i},{j}): {du:?} vs {:?}", d[i + 1][j]);
                assert!(close(dv, d[i][j + 1], 1e-5), "d/dv of ({i},{j}): {dv:?} vs {:?}", d[i][j + 1]);
            }
        }
    }

    #[test]
    fn a_rational_quarter_circle_has_the_circles_derivatives() {
        // On the circle of radius 2, the speed varies, but |r'| r'' and the curvature 1/2 do not:
        // check the curvature (|r' × r''| / |r'|³) and its derivative (zero) from the table.
        let s = quarter_cylinder();
        for u in [0.0, 0.2, 0.5, 0.9, 1.0] {
            let d = s.partials(u, 0.4).unwrap();
            assert!((d[0][0].x.hypot(d[0][0].y) - 2.0).abs() < 1e-12);
            let (r1, r2) = (d[1][0], d[2][0]);
            let k = r1.cross(r2).len() / r1.len().powi(3);
            assert!((k - 0.5).abs() < 1e-12, "{k}");
            assert!(d[0][2].len() < 1e-12 && d[1][1].z.abs() < 1e-12);
        }
    }

    #[test]
    fn hostile_parameters_give_none() {
        let s = quarter_cylinder();
        assert!(s.partials(f64::NAN, 0.0).is_none());
        assert!(s.partials(0.0, f64::INFINITY).is_none());
        let c = EvalCurve { degree: 2, knots: vec![0.0; 3], ctrl: vec![[0.0; 4]; 3] };
        assert!(c.derivatives(0.5, 2).is_none());
    }
}
