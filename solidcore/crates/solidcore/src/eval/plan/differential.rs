//! Analytic second derivatives of the shared rational and revolved evaluators.
use super::*;

/// A curve's value and its derivatives at one parameter.
#[derive(Clone, Copy, Debug)]
pub struct CurveJet {
    /// The point on the curve.
    pub point: Vec3,
    /// The first derivative with respect to the curve parameter.
    pub tangent: Vec3,
    /// The second derivative with respect to the curve parameter.
    pub second: Vec3,
}
/// A surface's value and its first and second partial derivatives.
#[derive(Clone, Copy, Debug)]
pub struct SurfaceJet {
    /// The point on the surface.
    pub point: Vec3,
    /// First partial derivative in U.
    pub du: Vec3,
    /// First partial derivative in V.
    pub dv: Vec3,
    /// Second partial derivative in U.
    pub duu: Vec3,
    /// Mixed second partial derivative in U and V.
    pub duv: Vec3,
    /// Second partial derivative in V.
    pub dvv: Vec3,
}
impl SurfaceJet {
    /// The unit normal in parameter orientation, if both tangents define a plane.
    pub fn normal(&self) -> Option<Vec3> {
        normal::normal(self.du, self.dv)
    }
    /// The second fundamental form on two world-space tangent directions.
    /// Unit orthogonal directions give entries of the full curvature tensor.
    pub fn curvature(&self, a: Vec3, b: Vec3) -> Option<f64> {
        if !a.is_finite() || !b.is_finite() {
            return None;
        }
        let (e, f, g) = (self.du.len2(), self.du.dot(self.dv), self.dv.len2());
        let determinant = e * g - f * f;
        if determinant <= 1e-16 * e * g || !determinant.is_finite() {
            return None;
        }
        let coefficients = |v: Vec3| {
            [
                (v.dot(self.du) * g - v.dot(self.dv) * f) / determinant,
                (v.dot(self.dv) * e - v.dot(self.du) * f) / determinant,
            ]
        };
        let ([au, av], [bu, bv]) = (coefficients(a), coefficients(b));
        let value = self
            .normal()?
            .dot(self.duu * (au * bu) + self.duv * (au * bv + av * bu) + self.dvv * (av * bv));
        value.is_finite().then_some(value)
    }
}
fn basis_jet(knots: &[f64], span: usize, p: usize, t: f64) -> Option<[[f64; W]; 3]> {
    let first = basis_ders(knots, span, p, t)?;
    let mut second = [0.0; W];
    if p > 1 {
        let [_, lower] = basis_ders(knots, span, p - 1, t)?;
        let start = span.checked_sub(p)?;
        for (j, value) in second.iter_mut().enumerate().take(p + 1) {
            let i = start.checked_add(j)?;
            let left = knots.get(i + p)? - knots.get(i)?;
            let right = knots.get(i + p + 1)? - knots.get(i + 1)?;
            let a = if j > 0 && left > 0.0 {
                lower.get(j - 1)? / left
            } else {
                0.0
            };
            let b = if j < p && right > 0.0 {
                lower.get(j)? / right
            } else {
                0.0
            };
            *value = p as f64 * (a - b);
        }
    }
    let [value, first] = first;
    Some([value, first, second])
}
fn valid(p: usize, n: usize, k: &[f64]) -> bool {
    (1..=MAX_SPLINE_DEGREE).contains(&p)
        && n > p
        && n <= MAX_EVAL_POINTS
        && n.checked_add(p).and_then(|n| n.checked_add(1)) == Some(k.len())
}
fn xyz(p: [f64; 4]) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}
impl EvalCurve {
    /// Point and two derivatives, including the polynomial continuation of end spans.
    pub fn jet(&self, t: f64) -> Option<CurveJet> {
        let (p, n) = (self.degree, self.ctrl.len());
        if !t.is_finite() || !valid(p, n, &self.knots) {
            return None;
        }
        let span = span_of(&self.knots, p, n, t);
        let basis = basis_jet(&self.knots, span, p, t)?;
        let mut h = [[0.0; 4]; 3];
        let cpts = self.ctrl.get(span.checked_sub(p)?..=span)?;
        for (j, c) in cpts.iter().enumerate() {
            for (h, b) in h.iter_mut().zip(&basis) {
                let weight = b.get(j)?;
                for (v, c) in h.iter_mut().zip(c) {
                    *v += c * weight;
                }
            }
        }
        let (point, tangent) = rational(h[0], h[1])?;
        let second = (xyz(h[2]) - tangent * (2.0 * h[1][3]) - point * h[2][3]) / h[0][3];
        second.is_finite().then_some(CurveJet {
            point,
            tangent,
            second,
        })
    }
}
impl EvalSurface {
    /// Analytic point and all first and second partial derivatives.
    pub fn jet(&self, u: f64, v: f64) -> Option<SurfaceJet> {
        if !u.is_finite() || !v.is_finite() {
            return None;
        }
        let jet = match self {
            Self::Plane { origin, du, dv } => SurfaceJet {
                point: *origin + *du * u + *dv * v,
                du: *du,
                dv: *dv,
                duu: Vec3::ZERO,
                duv: Vec3::ZERO,
                dvv: Vec3::ZERO,
            },
            Self::Nurbs {
                degree,
                knots,
                count,
                ctrl,
                ..
            } => {
                let [u, v] = self.wrap([u, v]);
                nurbs_jet(*degree, knots, *count, ctrl, u, v)?
            }
            Self::Revolved {
                profile,
                origin,
                axis,
                transform,
                swap,
            } => {
                let [u, v] = self.wrap([u, v]);
                let (s, a) = if *swap { (v, u) } else { (u, v) };
                let c = profile.jet(s)?;
                let (sin, cos) = a.sin_cos();
                let rotate =
                    |p: Vec3| p * cos + axis.cross(p) * sin + *axis * (axis.dot(p) * (1.0 - cos));
                let radius = rotate(c.point - *origin);
                let (ds, da) = (rotate(c.tangent), axis.cross(radius));
                let (dss, dsa, daa) = (rotate(c.second), axis.cross(ds), axis.cross(da));
                let (du, dv, duu, duv, dvv) = if *swap {
                    (da, ds, daa, dsa, dss)
                } else {
                    (ds, da, dss, dsa, daa)
                };
                SurfaceJet {
                    point: affine(transform, *origin + radius),
                    du: linear(transform, du),
                    dv: linear(transform, dv),
                    duu: linear(transform, duu),
                    duv: linear(transform, duv),
                    dvv: linear(transform, dvv),
                }
            }
        };
        [jet.point, jet.du, jet.dv, jet.duu, jet.duv, jet.dvv]
            .iter()
            .all(|v| v.is_finite())
            .then_some(jet)
    }
}
fn nurbs_jet(
    degree: [usize; 2],
    knots: &[Vec<f64>; 2],
    count: [usize; 2],
    ctrl: &[[f64; 4]],
    u: f64,
    v: f64,
) -> Option<SurfaceJet> {
    let [pu, pv] = degree;
    let [nu, nv] = count;
    if !valid(pu, nu, &knots[0])
        || !valid(pv, nv, &knots[1])
        || nu.checked_mul(nv)? != ctrl.len()
        || ctrl.len() > MAX_EVAL_POINTS
    {
        return None;
    }
    let (su, sv) = (span_of(&knots[0], pu, nu, u), span_of(&knots[1], pv, nv, v));
    let (bu, bv) = (
        basis_jet(&knots[0], su, pu, u)?,
        basis_jet(&knots[1], sv, pv, v)?,
    );
    let [u0, u1, u2] = bu;
    let [v0, v1, v2] = bv;
    let u_basis = u0.into_iter().zip(u1).zip(u2).map(|((a, b), c)| [a, b, c]);
    let v_basis = v0.into_iter().zip(v1).zip(v2).map(|((a, b), c)| [a, b, c]);
    let mut h = [[0.0; 4]; 6];
    let row = sv.checked_sub(pv)?;
    let col = su.checked_sub(pu)?;
    for (j, [v, dv, dvv]) in v_basis.enumerate().take(pv + 1) {
        for (i, [u, du, duu]) in u_basis.clone().enumerate().take(pu + 1) {
            let c = ctrl.get((row + j) * nu + col + i)?;
            let weights = [u * v, du * v, u * dv, duu * v, du * dv, u * dvv];
            for (out, weight) in h.iter_mut().zip(weights) {
                for (out, c) in out.iter_mut().zip(c) {
                    *out += weight * c;
                }
            }
        }
    }
    let (point, du) = rational(h[0], h[1])?;
    let (_, dv) = rational(h[0], h[2])?;
    let w = h[0][3];
    let duu = (xyz(h[3]) - du * (2.0 * h[1][3]) - point * h[3][3]) / w;
    let duv = (xyz(h[4]) - du * h[2][3] - dv * h[1][3] - point * h[4][3]) / w;
    let dvv = (xyz(h[5]) - dv * (2.0 * h[2][3]) - point * h[5][3]) / w;
    Some(SurfaceJet {
        point,
        du,
        dv,
        duu,
        duv,
        dvv,
    })
}
