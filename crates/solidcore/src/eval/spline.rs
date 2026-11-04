//! Splines and revolutions: a plane, a rational tensor B-spline and a profile turned about
//! an axis, with their points, partial derivatives to the fourth order and normals. The
//! B-splines are evaluated by NLib ([`NlSurface::derivs`], [`NlCurve::derivs`]); a plane is
//! closed form; a revolution turns its profile's derivatives by the rotation's own
//! (Rodrigues).
//!
//! Outside its knot interval a spline continues as the polynomial of its end span (a line
//! extends for ever), except along a direction in which it closes on itself
//! ([`super::uv::closed`]), where the parameter is taken a whole number of periods back
//! inside ([`EvalSurface::wrap`]).

#![allow(clippy::neg_cmp_op_on_partial_ord)] // NaN-refusing tests

use super::uv;
use super::vec::VecExt;
use super::{Curve2, EvalError};
use crate::math::Vec3;
use crate::nurbs::crv_basic::Curve as NlCurve;
use crate::nurbs::knots::Side;
use crate::nurbs::math::{CPoint, NOW, NOZ};
use crate::nurbs::srf_basic::Surface as NlSurface;
use crate::nurbs::srf_nurb::EuclidNet;

/// The highest spline degree the evaluator takes.
pub const MAX_SPLINE_DEGREE: usize = 16;
/// Most knots in one evaluable knot vector.
pub const MAX_EVAL_KNOTS: usize = 1 << 12;
/// Most control points in one evaluable surface.
pub const MAX_EVAL_POINTS: usize = 1 << 18;

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

/// Whether `knots` fit a spline of degree `p` over `n` control points the evaluator takes.
pub(crate) fn basis_check(p: usize, n: usize, knots: &[f64]) -> bool {
    (1..=MAX_SPLINE_DEGREE).contains(&p) && n > p && knots.len() == n + p + 1
}

/// How far past its knot range, relative to the range's magnitude, a parameter is still the
/// range's end for derivatives of the second order and up: rounding (a boundary point carried
/// off by a pcurve's evaluation), not an extrapolation.
const BOUNDARY_ROUNDING: f64 = 1e-9;

/// `t` on the knot range `[knots[p], knots[n]]`: the range's end when `t` is past it by no more
/// than rounding ([`BOUNDARY_ROUNDING`]), `None` when farther out (higher derivatives are taken
/// on the surface, never on its extrapolation), `t` inside.
pub(super) fn onto_knots(knots: &[f64], p: usize, n: usize, t: f64) -> Option<f64> {
    let (&lo, &hi) = (knots.get(p)?, knots.get(n)?);
    let slack = BOUNDARY_ROUNDING * (hi - lo).abs().max(lo.abs()).max(hi.abs()).max(1.0);
    if t < lo {
        (t >= lo - slack).then_some(lo)
    } else if t > hi {
        (t <= hi + slack).then_some(hi)
    } else {
        Some(t)
    }
}

/// NLib's control point for homogeneous `h`: non-rational (weight `NL_NOW`) when `rational` is
/// false.
fn cpoint(h: [f64; 4], rational: bool) -> CPoint {
    CPoint::new(h[0], h[1], h[2], if rational { h[3] } else { NOW })
}

/// Whether any homogeneous weight differs from 1.
fn rational(ctrl: &[[f64; 4]]) -> bool {
    ctrl.iter().any(|h| h[3] != 1.0)
}

/// NLib's 2D curve for a pcurve (`z` = `NL_NOZ`).
pub(crate) fn nl_curve2(c: &Curve2) -> Option<NlCurve> {
    let (knots, pw): (&[f64], Vec<CPoint>) = match c {
        Curve2::Spline { knots, points } => (
            knots,
            points
                .iter()
                .map(|&[u, v]| CPoint::new(u, v, NOZ, NOW))
                .collect(),
        ),
        Curve2::RationalSpline { knots, homogeneous } => (
            knots,
            homogeneous
                .iter()
                .map(|&[u, v, w]| CPoint::new(u, v, NOZ, w))
                .collect(),
        ),
    };
    let p = knots.len().checked_sub(pw.len() + 1)?;
    if !basis_check(p, pw.len(), knots) {
        return None;
    }
    NlCurve::new(pw, p, knots.to_vec()).ok()
}

/// A rational B-spline curve: homogeneous control points `[x·w, y·w, z·w, w]`. Outside its
/// domain it continues as the polynomial of the end span (a line extends for ever).
#[derive(Clone, Debug, PartialEq)]
pub struct EvalCurve {
    degree: usize,
    knots: Vec<f64>,
    ctrl: Vec<[f64; 4]>,
    /// NLib's curve, `None` when the data are malformed (nothing then evaluates).
    nl: Option<NlCurve>,
}

impl EvalCurve {
    /// The curve of `degree` on `knots` with homogeneous control points `ctrl` (all weights 1:
    /// a polynomial B-spline).
    #[must_use]
    pub fn new(degree: usize, knots: Vec<f64>, ctrl: Vec<[f64; 4]>) -> EvalCurve {
        let nl = basis_check(degree, ctrl.len(), &knots)
            .then(|| {
                let r = rational(&ctrl);
                NlCurve::new(
                    ctrl.iter().map(|h| cpoint(*h, r)).collect(),
                    degree,
                    knots.clone(),
                )
                .ok()
            })
            .flatten();
        EvalCurve {
            degree,
            knots,
            ctrl,
            nl,
        }
    }

    /// The degree.
    #[must_use]
    pub fn degree(&self) -> usize {
        self.degree
    }

    /// The knots.
    #[must_use]
    pub fn knots(&self) -> &[f64] {
        &self.knots
    }

    /// The homogeneous control points.
    #[must_use]
    pub fn ctrl(&self) -> &[[f64; 4]] {
        &self.ctrl
    }

    /// The active interval `[knots[degree], knots[count]]`.
    #[must_use]
    pub fn interval(&self) -> Option<(f64, f64)> {
        Some((
            *self.knots.get(self.degree)?,
            *self.knots.get(self.ctrl.len())?,
        ))
    }

    /// The point and derivatives up to `order` (at most [`MAX_PARTIAL_ORDER`]) at `t` from
    /// the side of `t` named (`Side::Left`: the span starting at `t` where `t` is a knot).
    fn derivs_from(&self, t: f64, order: usize, side: Side) -> Option<Vec<Vec3>> {
        if !t.is_finite() || order > MAX_PARTIAL_ORDER {
            return None;
        }
        let d = self.nl.as_ref()?.derivs_continued(t, side, order).ok()?;
        d.iter().all(|x| x.is_finite()).then_some(d)
    }

    /// Point and first derivative at `t`; `None` if the curve is malformed or its weight
    /// vanishes there.
    #[must_use]
    pub fn eval(&self, t: f64) -> Option<(Vec3, Vec3)> {
        match self.derivs_from(t, 1, Side::Left)?.as_slice() {
            [p, d] => Some((*p, *d)),
            _ => None,
        }
    }

    /// The first derivative at `t` from either side: on the knot spans that end and start
    /// there where `t` is a knot (the polynomials either side of it), else the derivative twice.
    #[must_use]
    pub fn sides(&self, t: f64) -> Option<(Vec3, Vec3)> {
        let left = self.derivs_from(t, 1, Side::Right)?;
        let right = self.derivs_from(t, 1, Side::Left)?;
        Some((*left.get(1)?, *right.get(1)?))
    }

    /// The point and derivatives up to `order` at `t` (element `k` the `k`-th), exactly. `None`
    /// if the curve is malformed or its weight vanishes there.
    #[must_use]
    /// Past the knot range by no more than rounding, `t` is the range's end; farther out,
    /// `None`.
    pub fn derivatives(&self, t: f64, order: usize) -> Option<Vec<Vec3>> {
        let t = onto_knots(&self.knots, self.degree, self.ctrl.len(), t)?;
        self.derivs_from(t, order, Side::Left)
    }
}

/// A rational tensor B-spline: control point `(i, j)` is `ctrl[j·count[0] + i]`.
#[derive(Clone, Debug, PartialEq)]
pub struct Nurbs {
    degree: [usize; 2],
    knots: [Vec<f64>; 2],
    count: [usize; 2],
    ctrl: Vec<[f64; 4]>,
    /// Whether it closes on itself along each direction ([`uv::closed`]), worked out once
    /// (every evaluation outside the knot interval asks, and the test walks the whole net).
    closed: [bool; 2],
    /// NLib's surface and its Euclidean net, `None` when the data are malformed (nothing
    /// then evaluates).
    nl: Option<(NlSurface, EuclidNet)>,
}

impl Nurbs {
    /// `uv` on the knot ranges ([`onto_knots`]).
    fn onto_knots(&self, [u, v]: [f64; 2]) -> Option<[f64; 2]> {
        let [k0, k1] = &self.knots;
        let ([p0, p1], [n0, n1]) = (self.degree, self.count);
        Some([onto_knots(k0, p0, n0, u)?, onto_knots(k1, p1, n1, v)?])
    }

    /// The degrees along u and v.
    #[must_use]
    pub fn degree(&self) -> [usize; 2] {
        self.degree
    }
    /// The knots along u and v.
    #[must_use]
    pub fn knots(&self) -> &[Vec<f64>; 2] {
        &self.knots
    }
    /// The control points along u and v.
    #[must_use]
    pub fn count(&self) -> [usize; 2] {
        self.count
    }
    /// The homogeneous control points, u fastest.
    #[must_use]
    pub fn ctrl(&self) -> &[[f64; 4]] {
        &self.ctrl
    }
    /// Whether it closes on itself along u and v.
    #[must_use]
    pub fn closed(&self) -> [bool; 2] {
        self.closed
    }
}

/// A surface in a form the evaluator (and a display evaluator) understands.
#[derive(Clone, Debug, PartialEq)]
pub enum EvalSurface {
    /// `origin + u·du + v·dv`.
    Plane {
        /// The point at (0, 0).
        origin: Vec3,
        /// `S_u`.
        du: Vec3,
        /// `S_v`.
        dv: Vec3,
    },
    /// A rational tensor B-spline ([`EvalSurface::nurbs`]).
    Nurbs(Box<Nurbs>),
    /// `profile(u)` turned by the angle `v` (right-handed) about the axis through `origin`
    /// along the unit `axis`, then mapped by the affine `transform` (row-major 3×4); with
    /// `swap` the parameters are exchanged first (the surface is evaluated at `(v, u)`).
    Revolved {
        /// The profile.
        profile: EvalCurve,
        /// A point of the axis.
        origin: Vec3,
        /// The unit axis.
        axis: Vec3,
        /// The affine map applied after the turn, row-major 3×4.
        transform: [[f64; 4]; 3],
        /// Whether u is the angle and v the profile's parameter.
        swap: bool,
    },
}

impl EvalSurface {
    /// The rational tensor B-spline of `degree` on `knots` over `count` control points
    /// (`ctrl[j·count[0] + i]`, homogeneous), with whether it closes on itself.
    #[must_use]
    pub fn nurbs(
        degree: [usize; 2],
        knots: [Vec<f64>; 2],
        count: [usize; 2],
        ctrl: Vec<[f64; 4]>,
    ) -> EvalSurface {
        let closed = uv::closed(degree, &knots, count, &ctrl);
        Self::nurbs_closed(degree, knots, count, ctrl, closed)
    }

    /// [`EvalSurface::nurbs`] whose closedness along u and v is known (`closed`, as
    /// [`super::uv::closed`] finds it): a net moved rigidly or by an affine map keeps that of
    /// the net it was moved from, without walking the net again.
    #[must_use]
    pub fn nurbs_closed(
        degree: [usize; 2],
        knots: [Vec<f64>; 2],
        count: [usize; 2],
        ctrl: Vec<[f64; 4]>,
        closed: [bool; 2],
    ) -> EvalSurface {
        let [pu, pv] = degree;
        let [nu, nv] = count;
        let ok = basis_check(pu, nu, &knots[0])
            && basis_check(pv, nv, &knots[1])
            && nu.checked_mul(nv) == Some(ctrl.len());
        let nl = ok
            .then(|| {
                let r = rational(&ctrl);
                // NLib's net: rows along u, `net[i][j]`.
                let net: Option<Vec<Vec<CPoint>>> = (0..nu)
                    .map(|i| {
                        (0..nv)
                            .map(|j| ctrl.get(j * nu + i).map(|h| cpoint(*h, r)))
                            .collect()
                    })
                    .collect();
                let nl = NlSurface::new(net?, pu, pv, knots[0].clone(), knots[1].clone()).ok()?;
                let euclid = nl.euclid_net().ok()?;
                Some((nl, euclid))
            })
            .flatten();
        EvalSurface::Nurbs(Box::new(Nurbs {
            degree,
            knots,
            count,
            ctrl,
            closed,
            nl,
        }))
    }

    /// Point and unit normal (`S_u × S_v`, oriented as the surface) at `(u, v)`. At a
    /// degenerate point (a cone's apex, a sphere's pole) the normal is the limit there from
    /// the second derivatives, oriented by NLib's rule. [`EvalError::Unevaluable`] where the
    /// surface cannot be evaluated, [`EvalError::Degenerate`] where it has no normal (the limit
    /// vanishes too).
    pub fn eval(&self, u: f64, v: f64) -> Result<(Vec3, Vec3), EvalError> {
        let unevaluable = || EvalError::Unevaluable(format!("the surface at ({u}, {v})"));
        let (p, du, dv) = self.derivs(u, v).ok_or_else(unevaluable)?;
        let n = self.oriented(du.cross(dv));
        if let Some(n) = n
            .normalized()
            .filter(|_| n.len2() > 1e-20 * (du.len2() * dv.len2()).max(1e-300))
        {
            return Ok((p, n));
        }
        // The limit at the pole, by NLib's rule (N_SrfEvalPtNormalAtPole): where S_u vanishes,
        // S_uv × S_v, with S_v turned round at the end of v (S_u ≈ S_uv (v − v_end) there);
        // where S_v vanishes, S_u × S_uv, with S_u turned round at the end of u. Where that
        // vanishes too, there is no normal (NLib's NL_GEO_ERR).
        let at_end = |d: usize, t: f64| self.interval(d).is_some_and(|(_, hi)| t >= hi);
        let (_, suv, _) = self.second(u, v).ok_or_else(unevaluable)?;
        let m = if du.len2() <= dv.len2() {
            suv.cross(if at_end(1, v) { -dv } else { dv })
        } else {
            (if at_end(0, u) { -du } else { du }).cross(suv)
        };
        let n = self
            .oriented(m)
            .normalized()
            .ok_or_else(|| EvalError::Degenerate(format!("no normal at the pole ({u}, {v})")))?;
        Ok((p, n))
    }

    /// The unit normal at `(u, v)` approached along the parameter direction `dir`, oriented as
    /// the surface. Whether `(u, v)` is a pole, exactly or within `reach` mm (a row an importer
    /// left coinciding only to its tolerance), is the one classifier's ([`super::collapse`]):
    /// off a pole it is the normal ([`EvalSurface::eval`]); at one, the partials along each
    /// collapsed direction are set to exact zero and it is the first coefficient of the
    /// expansion of `S_u × S_v` along `dir` that does not vanish by cancellation
    /// ([`super::pole::limit_along`]), and [`EvalError::Degenerate`] at a pole of order above
    /// three.
    pub fn normal_along(
        &self,
        u: f64,
        v: f64,
        dir: [f64; 2],
        reach: f64,
    ) -> Result<Vec3, EvalError> {
        Ok(self.limit_along(u, v, dir, reach)?.0)
    }

    /// [`Self::normal_along`] with the order `k` of the coefficient it is (0 off a pole).
    pub fn limit_along(
        &self,
        u: f64,
        v: f64,
        dir: [f64; 2],
        reach: f64,
    ) -> Result<(Vec3, usize), EvalError> {
        let c = super::collapse::collapse(self, [u, v], reach)?;
        if c == super::collapse::Collapse::Neither {
            return Ok((self.eval(u, v)?.1, 0));
        }
        let (mut d, mut errors) = super::collapse::certified_partials(self, [u, v])?;
        super::pole::zero_collapsed(&mut d, &mut errors, c.flags());
        let (n, k) = super::pole::limit_along(&d, dir, &errors)?;
        Ok((self.oriented(n), k))
    }

    /// The active parameter interval along `d` (0: u, 1: v) of a spline, or of a revolution's
    /// profile; `None` where the parameter is unbounded (a plane, a revolution's angle).
    #[must_use]
    pub fn interval(&self, d: usize) -> Option<(f64, f64)> {
        match self {
            EvalSurface::Plane { .. } => None,
            EvalSurface::Nurbs(n) => Some((
                *n.knots.get(d)?.get(*n.degree.get(d)?)?,
                *n.knots.get(d)?.get(*n.count.get(d)?)?,
            )),
            EvalSurface::Revolved { profile, swap, .. } => (d == usize::from(*swap))
                .then(|| profile.interval())
                .flatten(),
        }
    }

    /// Mirror transforms reverse the cross product of the mapped derivatives.
    fn oriented(&self, n: Vec3) -> Vec3 {
        match self {
            EvalSurface::Revolved { transform: m, .. } if det3(m) < 0.0 => -n,
            _ => n,
        }
    }

    /// `uv` with each parameter along which a spline closes on itself (or a revolution's
    /// profile does) moved by whole periods into the active knot interval (unchanged inside it,
    /// and for any other surface).
    #[must_use]
    pub fn wrap(&self, uv: [f64; 2]) -> [f64; 2] {
        if let EvalSurface::Revolved { profile, swap, .. } = self {
            let d = usize::from(*swap);
            let range = self.interval(d);
            let mut out = uv;
            if let (Some(r), Some(x)) = (range, out.get_mut(d)) {
                if !(r.0..=r.1).contains(x) && uv::closed_curve(profile) {
                    *x = uv::wrap_into(*x, r);
                }
            }
            return out;
        }
        let EvalSurface::Nurbs(n) = self else {
            return uv;
        };
        let mut out = uv;
        for (d, (x, shut)) in out.iter_mut().zip(n.closed).enumerate() {
            if let (true, Some(r)) = (shut, self.interval(d)) {
                if !(r.0..=r.1).contains(x) {
                    *x = uv::wrap_into(*x, r);
                }
            }
        }
        out
    }

    /// Point and first partial derivatives (a closed spline's parameters wrapped into its
    /// active interval first, [`EvalSurface::wrap`]).
    #[must_use]
    pub fn derivs(&self, u: f64, v: f64) -> Option<(Vec3, Vec3, Vec3)> {
        if !(u.is_finite() && v.is_finite()) {
            return None;
        }
        match self {
            EvalSurface::Plane { origin, du, dv } => Some((*origin + *du * u + *dv * v, *du, *dv)),
            EvalSurface::Nurbs(n) => {
                let [u, v] = self.wrap([u, v]);
                let mut d = [[Vec3::ZERO; 2]; 2];
                let (nl, euclid) = n.nl.as_ref()?;
                nl.first_derivs_net_into(Some(euclid), u, v, Side::Left, Side::Left, true, &mut d)
                    .ok()?;
                let [[p, sv], [su, _]] = d;
                (p.is_finite() && su.is_finite() && sv.is_finite()).then_some((p, su, sv))
            }
            EvalSurface::Revolved {
                profile,
                origin,
                axis,
                transform,
                swap,
            } => {
                let [u, v] = self.wrap([u, v]);
                let (s, a) = if *swap { (v, u) } else { (u, v) };
                let (c, dc) = profile.eval(s)?;
                let (sin, cos) = a.sin_cos();
                let rot =
                    |x: Vec3| x * cos + axis.cross(x) * sin + *axis * (axis.dot(x) * (1.0 - cos));
                let r = c - *origin;
                let p = *origin + rot(r);
                let ds = rot(dc);
                // d/dθ of the rotated point: axis × (rotated radius).
                let da = axis.cross(rot(r));
                let (p, ds, da) = (
                    affine(transform, p),
                    linear(transform, ds),
                    linear(transform, da),
                );
                Some(if *swap { (p, da, ds) } else { (p, ds, da) })
            }
        }
    }

    /// The second partial derivatives `(S_uu, S_uv, S_vv)` at (u, v).
    #[must_use]
    pub fn second(&self, u: f64, v: f64) -> Option<(Vec3, Vec3, Vec3)> {
        if !(u.is_finite() && v.is_finite()) {
            return None;
        }
        match self {
            EvalSurface::Plane { .. } => Some((Vec3::ZERO, Vec3::ZERO, Vec3::ZERO)),
            EvalSurface::Nurbs(n) => {
                let [u, v] = n.onto_knots(self.wrap([u, v]))?;
                let d =
                    n.nl.as_ref()?
                        .0
                        .derivs(u, v, Side::Left, Side::Left, true, 2, 2)
                        .ok()?;
                let at = |i: usize, j: usize| {
                    d.get(i)
                        .and_then(|r| r.get(j))
                        .copied()
                        .filter(|x| x.is_finite())
                };
                Some((at(2, 0)?, at(1, 1)?, at(0, 2)?))
            }
            EvalSurface::Revolved { .. } => {
                let d = self.partials(u, v)?;
                Some((d[2][0], d[1][1], d[0][2]))
            }
        }
    }

    /// The partial derivatives at (u, v) up to the fourth order ([`Partials`]), exactly: a closed
    /// spline's parameters wrapped first as in [`EvalSurface::derivs`]. `None` where the surface
    /// cannot be evaluated (a malformed spline, a vanishing weight).
    #[must_use]
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
            EvalSurface::Nurbs(n) => {
                let [u, v] = n.onto_knots(self.wrap([u, v]))?;
                let d =
                    n.nl.as_ref()?
                        .0
                        .derivs(
                            u,
                            v,
                            Side::Left,
                            Side::Left,
                            true,
                            MAX_PARTIAL_ORDER,
                            MAX_PARTIAL_ORDER,
                        )
                        .ok()?;
                tabulate(|i, j| d.get(i)?.get(j).copied())
            }
            EvalSurface::Revolved {
                profile,
                origin,
                axis,
                transform,
                swap,
            } => {
                let [u, v] = self.wrap([u, v]);
                let (s, a) = if *swap { (v, u) } else { (u, v) };
                let c = profile.derivatives(s, MAX_PARTIAL_ORDER)?;
                let turned = |i: usize, j: usize| -> Option<Vec3> {
                    // The profile's i-th derivative (its point taken about the axis), turned by
                    // the j-th derivative of the rotation.
                    let x = if i == 0 {
                        *c.first()? - *origin
                    } else {
                        *c.get(i)?
                    };
                    let r = rotation_derivative(*axis, x, a, j);
                    Some(if i == 0 && j == 0 {
                        affine(transform, *origin + r)
                    } else {
                        linear(transform, r)
                    })
                };
                tabulate(|i, j| if *swap { turned(j, i) } else { turned(i, j) })
            }
        }
    }
}

/// The `j`-th derivative by the angle `a` of `x` turned by `a` about the unit `axis` (Rodrigues:
/// `x cos a + (axis × x) sin a + axis (axis · x)(1 − cos a)`).
pub(super) fn rotation_derivative(axis: Vec3, x: Vec3, a: f64, j: usize) -> Vec3 {
    let (cos, sin) = trig_derivative(a, j);
    let along = axis * axis.dot(x);
    let constant = if j == 0 { along } else { Vec3::ZERO };
    x * cos + axis.cross(x) * sin - along * cos + constant
}

/// The affine map `m` (row-major 3×4) of a point.
pub(crate) fn affine(m: &[[f64; 4]; 3], p: Vec3) -> Vec3 {
    linear(m, p) + Vec3::new(m[0][3], m[1][3], m[2][3])
}

/// The linear part of `m` applied to a vector.
pub(crate) fn linear(m: &[[f64; 4]; 3], v: Vec3) -> Vec3 {
    let row = |r: &[f64; 4]| r[0] * v.x + r[1] * v.y + r[2] * v.z;
    Vec3::new(row(&m[0]), row(&m[1]), row(&m[2]))
}

fn det3(m: &[[f64; 4]; 3]) -> f64 {
    m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
}
