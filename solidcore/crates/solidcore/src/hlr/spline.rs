//! The spline work hidden lines need, on the kernel's own: a curve's part between two
//! parameters and its Bézier pieces (the kernel's knot insertion, segment extraction and Bézier
//! decomposition), a Bézier piece's part (its segment extraction), the binomials of Bernstein
//! products, and a rational spline surface's points (the evaluator).

use crate::eval::{Surface, SurfaceEval};
use crate::math::Vec3;
use crate::nurbs::crv_basic::Curve as NlCurve;
use crate::nurbs::math::{CPoint, NOW, NOZ};

/// A homogeneous control point `[x w, y w, z w, w]`.
pub(crate) type Hom = [f64; 4];

fn cpoint(h: &Hom) -> CPoint {
    CPoint {
        x: h[0],
        y: h[1],
        z: h[2],
        w: h[3],
    }
}

fn hom(c: &CPoint) -> Hom {
    let z = if c.z == NOZ { 0.0 } else { c.z };
    if c.w == NOW {
        [c.x, c.y, z, 1.0]
    } else {
        [c.x, c.y, z, c.w]
    }
}

/// The Cartesian point of `h`, where its weight is usable.
pub(crate) fn project(h: &Hom) -> Option<Vec3> {
    let p = Vec3::new(h[0] / h[3], h[1] / h[3], h[2] / h[3]);
    (h[3].abs() > f64::MIN_POSITIVE && p.x.is_finite() && p.y.is_finite() && p.z.is_finite())
        .then_some(p)
}

/// `C(n, k)`.
pub(crate) fn binomial(n: usize, k: usize) -> f64 {
    if k > n {
        return 0.0;
    }
    crate::blend::fillet_section::form::binom(n, k)
}

/// The part of the degree-`p` spline on `knots` and `ctrl` over `[a, b]`, within its domain: its
/// knots and control points ([`NlCurve::extract_seg`]).
pub(crate) fn segment(
    p: usize,
    knots: &[f64],
    ctrl: &[Hom],
    a: f64,
    b: f64,
) -> Option<(Vec<f64>, Vec<Hom>)> {
    let curve = NlCurve::new(ctrl.iter().map(cpoint).collect(), p, knots.to_vec()).ok()?;
    let part = curve.extract_seg(a, b).ok()?;
    Some((part.knots().to_vec(), part.cpts().iter().map(hom).collect()))
}

/// The control points of the part of the Bézier curve `ctrl` over `[a, b] ⊂ [0, 1]`.
pub(crate) fn bezier_segment(ctrl: &[Hom], a: f64, b: f64) -> Option<Vec<Hom>> {
    if !(a.is_finite() && b.is_finite() && 0.0 <= a && a < b && b <= 1.0) {
        return None;
    }
    let p = ctrl.len().checked_sub(1).filter(|p| *p > 0)?;
    let mut knots = vec![0.0; p + 1];
    knots.extend(std::iter::repeat_n(1.0, p + 1));
    if a == 0.0 && b == 1.0 {
        return Some(ctrl.to_vec());
    }
    Some(segment(p, &knots, ctrl, a, b)?.1)
}

/// The Bézier pieces of the spline on `knots` and `ctrl` over its whole domain, each with the
/// span of the parameter it covers ([`crate::eval::query::bezier_spans`]); empty when it isn't a
/// curve.
pub(crate) fn pieces(knots: &[f64], ctrl: &[Hom]) -> Vec<([f64; 2], Vec<Hom>)> {
    crate::eval::query::bezier_spans(knots, ctrl, [f64::NEG_INFINITY, f64::INFINITY])
        .unwrap_or_default()
}

/// A rational spline surface: its control net (u fastest), degrees, knots and evaluator.
#[derive(Clone, Debug)]
pub(crate) struct NurbsSurface {
    ctrl: Vec<Hom>,
    degree: [usize; 2],
    knots: [Vec<f64>; 2],
    eval: SurfaceEval,
}

impl NurbsSurface {
    /// The surface of `rows` (`rows[i][j]`, i along u) on knots `u` and `v`; `None` when the
    /// evaluator refuses it.
    pub(crate) fn new(u: &[f64], v: &[f64], rows: &[Vec<Hom>]) -> Option<NurbsSurface> {
        let nu = rows.len();
        let nv = rows.first().map_or(0, Vec::len);
        let pu = u.len().checked_sub(nu + 1)?;
        let pv = v.len().checked_sub(nv + 1)?;
        if pu == 0 || pv == 0 || rows.iter().any(|r| r.len() != nv) {
            return None;
        }
        if rows
            .iter()
            .flatten()
            .any(|h| h.iter().any(|x| !x.is_finite()) || !(h[3] > 0.0))
        {
            return None;
        }
        let mut ctrl = Vec::new();
        for j in 0..nv {
            for row in rows {
                ctrl.push(*row.get(j)?);
            }
        }
        let eval = SurfaceEval::new(&Surface::RationalSpline {
            u_knots: u.to_vec(),
            v_knots: v.to_vec(),
            homogeneous: rows.to_vec(),
        })
        .ok()?;
        Some(NurbsSurface {
            ctrl,
            degree: [pu, pv],
            knots: [u.to_vec(), v.to_vec()],
            eval,
        })
    }
    /// The control points, u fastest.
    pub(crate) fn homogeneous(&self) -> &[Hom] {
        &self.ctrl
    }
    /// The degrees along u and v.
    pub(crate) fn degree(&self) -> [usize; 2] {
        self.degree
    }
    /// The knots along u and v.
    pub(crate) fn knots(&self) -> &[Vec<f64>; 2] {
        &self.knots
    }
    /// `[[u0, u1], [v0, v1]]`: from the degree's knot to the count's.
    pub(crate) fn domain(&self) -> [[f64; 2]; 2] {
        let d = |k: usize| {
            let p = self.degree.get(k).copied().unwrap_or(0);
            let kn = self.knots.get(k).map_or(&[][..], Vec::as_slice);
            let n = kn.len().saturating_sub(p + 1);
            [
                kn.get(p).copied().unwrap_or(0.0),
                kn.get(n).copied().unwrap_or(0.0),
            ]
        };
        [d(0), d(1)]
    }
    /// The point at `(u, v)`.
    pub(crate) fn eval(&self, u: f64, v: f64) -> Option<Vec3> {
        self.eval.point([u, v])
    }
    /// The point at `(u, v)` and its partials along u and v, in one evaluation.
    pub(crate) fn eval_d1(&self, u: f64, v: f64) -> Option<[Vec3; 3]> {
        let (p, su, sv) = self.eval.derivs(u, v)?;
        Some([p, su, sv])
    }
}
