//! Display tessellation plans: the parameter-space triangulation of every face of a body,
//! with the surfaces to evaluate it on, kept apart from the evaluation itself.
//!
//! The kernel decides *where* the vertices go (in each face's (u, v) parameters, with the
//! shared edge nodes fixed in 3D so neighbouring faces meet exactly) and *how* they are
//! connected. Evaluating the surfaces at those parameters is the bulk of the work and has no
//! branching on topology, so it can run anywhere: [`TessPlan::evaluate`] does it on the CPU,
//! and the GPU evaluator runs the same maths in a compute shader. Both evaluate the
//! [`EvalSurface`] forms defined here, so the two cannot drift apart.
//!
//! References: Piegl & Tiller, *The NURBS Book* (2nd ed.), A2.1 (span), A2.3 (basis
//! functions and derivatives), §4.5 (derivatives of a rational surface). [`uv`] finds the
//! parameters of a point on an evaluator surface.

use crate::bspline::MAX_SPLINE_DEGREE;
use crate::{Mesh, Vec3};

pub mod partials;
pub mod uv;

/// Most knots in one evaluable knot vector (bounds the shader's span search).
pub const MAX_EVAL_KNOTS: usize = 1 << 12;
/// Most control points in one evaluable surface.
pub const MAX_EVAL_POINTS: usize = 1 << 18;
/// Basis slots for the highest evaluable degree.
pub(crate) const W: usize = MAX_SPLINE_DEGREE + 1;

/// A rational B-spline curve: homogeneous control points `[x·w, y·w, z·w, w]`. Outside its
/// domain it continues as the polynomial of the end span (a line extends for ever).
#[derive(Clone, Debug, PartialEq)]
pub struct EvalCurve {
    pub degree: usize,
    pub knots: Vec<f64>,
    pub ctrl: Vec<[f64; 4]>,
}

/// A surface in a form both evaluators understand.
#[derive(Clone, Debug, PartialEq)]
pub enum EvalSurface {
    /// `origin + u·du + v·dv`.
    Plane { origin: Vec3, du: Vec3, dv: Vec3 },
    /// A rational tensor B-spline; control point `(i, j)` is `ctrl[j·count[0] + i]`. `closed`:
    /// whether it closes on itself along each direction ([`uv::closed`]), worked out once by
    /// [`EvalSurface::nurbs`] (every evaluation outside the knot interval asks, and the test
    /// walks the whole control net).
    Nurbs { degree: [usize; 2], knots: [Vec<f64>; 2], count: [usize; 2], ctrl: Vec<[f64; 4]>, closed: [bool; 2] },
    /// `profile(u)` turned by the angle `v` (right-handed) about the axis through `origin`
    /// along the unit `axis`, then mapped by the affine `transform` (row-major 3×4); with
    /// `swap` the parameters are exchanged first (the surface is evaluated at `(v, u)`).
    Revolved { profile: EvalCurve, origin: Vec3, axis: Vec3, transform: [[f64; 4]; 3], swap: bool },
}

/// One face's share of a [`TessPlan`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FacePlan {
    /// The B-rep face (index in the body's face order).
    pub face: u32,
    /// Index into [`TessPlan::surfaces`]; `None` when every position and normal is given.
    pub surface: Option<u32>,
    /// Parameters of every vertex.
    pub uv: Vec<[f64; 2]>,
    /// Positions of the first `fixed.len()` vertices: the shared edge nodes, used exactly as
    /// they are so the neighbouring face gets bit-identical boundary points.
    pub fixed: Vec<Vec3>,
    /// Normals of every vertex, only for faces without an evaluable surface.
    pub normals: Vec<Vec3>,
    /// The face is reversed against its surface: normals point the other way.
    pub flip: bool,
    /// Counter-clockwise seen from outside (orientation already applied).
    pub triangles: Vec<[u32; 3]>,
}

impl FacePlan {
    /// Triangle `t` is on this face's own nodes (the evaluated mesh keeps only those).
    fn has_nodes(&self, t: &[u32; 3]) -> bool {
        t.iter().all(|&i| (i as usize) < self.uv.len())
    }
}

/// A body's display tessellation, ready to evaluate.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TessPlan {
    pub surfaces: Vec<EvalSurface>,
    pub faces: Vec<FacePlan>,
    /// B-rep edges as polylines (the shared edge nodes), in the body's edge order.
    pub edges: Vec<Vec<Vec3>>,
    /// Per edge: the faces it bounds.
    pub edge_faces: Vec<Vec<u32>>,
}

impl TessPlan {
    /// Evaluate on the CPU into an indexed mesh (vertices are shared within a face, not
    /// across faces, so creases keep their own normals).
    pub fn evaluate(&self) -> Mesh {
        let mut out = Mesh::default();
        for f in &self.faces {
            let base = out.positions.len();
            let surface = f.surface.and_then(|i| self.surfaces.get(i as usize));
            for (k, uv) in f.uv.iter().enumerate() {
                let (p, n) = match surface {
                    Some(s) => s.eval(uv[0], uv[1]),
                    None => (Vec3::ZERO, f.normals.get(k).copied().unwrap_or(Vec3::Z)),
                };
                let p = f.fixed.get(k).copied().unwrap_or(p);
                out.positions.push(p);
                out.normals.push(if f.flip { -n } else { n });
            }
            for t in &f.triangles {
                if f.has_nodes(t) {
                    let at = |i: u32| u32::try_from(base + i as usize).unwrap_or(u32::MAX);
                    out.triangles.push([at(t[0]), at(t[1]), at(t[2])]);
                    out.tri_face.push(f.face);
                }
            }
        }
        out.edges = self.edges.clone();
        out.edge_faces = self.edge_faces.clone();
        out
    }

    /// Which of the body's `faces` faces have triangles in [`TessPlan::evaluate`]'s mesh,
    /// without evaluating it.
    pub fn faces_with_triangles(&self, faces: usize) -> Vec<bool> {
        let mut has = vec![false; faces];
        for f in &self.faces {
            if f.triangles.iter().any(|t| f.has_nodes(t))
                && let Some(h) = has.get_mut(f.face as usize)
            {
                *h = true;
            }
        }
        has
    }

    /// Vertices across all faces.
    pub fn vertex_count(&self) -> usize {
        self.faces.iter().map(|f| f.uv.len()).sum()
    }

    /// This plan for the body moved by the rotation and translation `m` (by columns, the
    /// translation in `m[3]`): the same parameters and triangles on the moved surfaces, so a
    /// part placed many times is planned once. Only for proper rigid motions (a reflection
    /// would turn the triangles inside out; the caller checks).
    pub fn transformed(&self, m: &[[f64; 4]; 4]) -> TessPlan {
        let vector = |v: Vec3| {
            Vec3::new(
                m[0][0] * v.x + m[1][0] * v.y + m[2][0] * v.z,
                m[0][1] * v.x + m[1][1] * v.y + m[2][1] * v.z,
                m[0][2] * v.x + m[1][2] * v.y + m[2][2] * v.z,
            )
        };
        let point = |p: Vec3| vector(p) + Vec3::new(m[3][0], m[3][1], m[3][2]);
        let surfaces = self
            .surfaces
            .iter()
            .map(|s| match s {
                EvalSurface::Plane { origin, du, dv } => EvalSurface::Plane { origin: point(*origin), du: vector(*du), dv: vector(*dv) },
                EvalSurface::Nurbs { degree, knots, count, ctrl, closed } => EvalSurface::Nurbs {
                    degree: *degree,
                    // An affine map keeps a surface closed or open.
                    closed: *closed,
                    knots: knots.clone(),
                    count: *count,
                    // Homogeneous: the translation scales with the weight.
                    ctrl: ctrl
                        .iter()
                        .map(|c| {
                            let q = vector(Vec3::new(c[0], c[1], c[2])) + Vec3::new(m[3][0], m[3][1], m[3][2]) * c[3];
                            [q.x, q.y, q.z, c[3]]
                        })
                        .collect(),
                },
                EvalSurface::Revolved { profile, origin, axis, transform, swap } => {
                    // `m` (as rows) after the surface's own affine map.
                    let r = |i: usize| [m[0][i], m[1][i], m[2][i], m[3][i]];
                    let rows = [r(0), r(1), r(2)];
                    let t = transform;
                    let composed = rows.map(|row| {
                        let mut out = [0.0; 4];
                        for (j, slot) in out.iter_mut().enumerate() {
                            *slot = (0..3).map(|k| row[k] * t[k][j]).sum::<f64>() + if j == 3 { row[3] } else { 0.0 };
                        }
                        out
                    });
                    EvalSurface::Revolved { profile: profile.clone(), origin: *origin, axis: *axis, transform: composed, swap: *swap }
                }
            })
            .collect();
        let faces = self
            .faces
            .iter()
            .map(|f| FacePlan {
                fixed: f.fixed.iter().map(|p| point(*p)).collect(),
                normals: f.normals.iter().map(|n| vector(*n)).collect(),
                ..f.clone()
            })
            .collect();
        TessPlan {
            surfaces,
            faces,
            edges: self.edges.iter().map(|e| e.iter().map(|p| point(*p)).collect()).collect(),
            edge_faces: self.edge_faces.clone(),
        }
    }
}

impl EvalCurve {
    /// Point and first derivative at `t`; `None` if the curve is malformed or its weight
    /// vanishes there.
    pub fn eval(&self, t: f64) -> Option<(Vec3, Vec3)> {
        let p = self.degree;
        let n = self.ctrl.len();
        if p == 0 || p > MAX_SPLINE_DEGREE || n <= p || self.knots.len() != n + p + 1 || !t.is_finite() {
            return None;
        }
        self.eval_on(span_of(&self.knots, p, n, t), t)
    }

    /// The first derivative at `t` from either side: on the knot spans that end and start
    /// there where `t` is a knot (the polynomials either side of it), else the derivative twice.
    pub fn sides(&self, t: f64) -> Option<(Vec3, Vec3)> {
        let p = self.degree;
        let n = self.ctrl.len();
        if p == 0 || p > MAX_SPLINE_DEGREE || n <= p || self.knots.len() != n + p + 1 || !t.is_finite() {
            return None;
        }
        let right = span_of(&self.knots, p, n, t);
        // The last non-empty span before `t`'s, when `t` is where it starts.
        let left = match self.knots.get(right) {
            Some(&k) if k == t => {
                (p..right).rev().find(|&i| matches!((self.knots.get(i), self.knots.get(i + 1)), (Some(a), Some(b)) if a < b)).unwrap_or(right)
            }
            _ => right,
        };
        Some((self.eval_on(left, t)?.1, self.eval_on(right, t)?.1))
    }

    /// Point and first derivative at `t` of the polynomial on knot span `span`.
    fn eval_on(&self, span: usize, t: f64) -> Option<(Vec3, Vec3)> {
        let p = self.degree;
        let ders = basis_ders(&self.knots, span, p, t)?;
        let (mut a, mut da) = ([0.0; 4], [0.0; 4]);
        for (k, (n0, n1)) in ders[0].iter().zip(&ders[1]).take(p + 1).enumerate() {
            let c = self.ctrl.get(span + k - p)?;
            for d in 0..4 {
                a[d] += n0 * c[d];
                da[d] += n1 * c[d];
            }
        }
        rational(a, da)
    }
}

impl EvalSurface {
    /// The rational tensor B-spline [`EvalSurface::Nurbs`], with whether it closes on itself.
    pub fn nurbs(degree: [usize; 2], knots: [Vec<f64>; 2], count: [usize; 2], ctrl: Vec<[f64; 4]>) -> EvalSurface {
        let closed = uv::closed(degree, &knots, count, &ctrl);
        EvalSurface::Nurbs { degree, knots, count, ctrl, closed }
    }

    /// Point and unit normal (`S_u × S_v`, oriented as the surface) at `(u, v)`. A degenerate
    /// point (a cone's apex, a sphere's pole) takes its normal from a step inside; anything
    /// that still fails gives the origin and +Z rather than a NaN.
    pub fn eval(&self, u: f64, v: f64) -> (Vec3, Vec3) {
        let Some((p, du, dv)) = self.derivs(u, v) else { return (Vec3::ZERO, Vec3::Z) };
        let n = self.oriented(du.cross(dv));
        if let Some(n) = n.normalized().filter(|_| n.len2() > 1e-20 * (du.len2() * dv.len2()).max(1e-300)) {
            return (p, n);
        }
        // Step into the parameter domain around the degenerate point: never past the end of an
        // interval, where the extrapolated surface folds back over the pole and its normal
        // points the other way.
        let h = 1e-6 * (1.0 + u.abs().max(v.abs()));
        let inside = |d: usize, t: f64, s: f64| self.interval(d).is_none_or(|(lo, hi)| (lo..=hi).contains(&(t + s * h)));
        for (su, sv) in [(1.0, 1.0), (-1.0, 1.0), (1.0, -1.0), (-1.0, -1.0)] {
            if inside(0, u, su)
                && inside(1, v, sv)
                && let Some((_, a, b)) = self.derivs(u + su * h, v + sv * h)
                && let Some(n) = self.oriented(a.cross(b)).normalized()
            {
                return (p, n);
            }
        }
        (p, Vec3::Z)
    }

    /// The active parameter interval along `d` (0: u, 1: v) of a spline, or of a revolution's
    /// profile; `None` where the parameter is unbounded (a plane, a revolution's angle).
    fn interval(&self, d: usize) -> Option<(f64, f64)> {
        match self {
            EvalSurface::Plane { .. } => None,
            EvalSurface::Nurbs { degree, knots, count, .. } => Some((*knots.get(d)?.get(*degree.get(d)?)?, *knots.get(d)?.get(*count.get(d)?)?)),
            EvalSurface::Revolved { profile, swap, .. } => {
                (d == usize::from(*swap)).then(|| Some((*profile.knots.get(profile.degree)?, *profile.knots.get(profile.ctrl.len())?))).flatten()
            }
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
    /// and for any other surface): the parameters both evaluators take for a point of a closed
    /// spline found a period away.
    pub fn wrap(&self, uv: [f64; 2]) -> [f64; 2] {
        if let EvalSurface::Revolved { profile, swap, .. } = self {
            let d = usize::from(*swap);
            let range = self.interval(d);
            let mut out = uv;
            if let (Some(r), Some(x)) = (range, out.get_mut(d))
                && !(r.0..=r.1).contains(x)
                && uv::closed_curve(profile)
            {
                *x = uv::wrap_into(*x, r);
            }
            return out;
        }
        let EvalSurface::Nurbs { closed, .. } = self else { return uv };
        let mut out = uv;
        for d in 0..2 {
            if let (true, Some(r)) = (closed[d], self.interval(d))
                && !(r.0..=r.1).contains(&uv[d])
            {
                out[d] = uv::wrap_into(uv[d], r);
            }
        }
        out
    }

    /// Point and first partial derivatives (a closed spline's parameters wrapped into its
    /// active interval first, [`EvalSurface::wrap`]).
    pub fn derivs(&self, u: f64, v: f64) -> Option<(Vec3, Vec3, Vec3)> {
        if !(u.is_finite() && v.is_finite()) {
            return None;
        }
        match self {
            EvalSurface::Plane { origin, du, dv } => Some((*origin + *du * u + *dv * v, *du, *dv)),
            EvalSurface::Nurbs { degree, knots, count, ctrl, .. } => {
                let [u, v] = self.wrap([u, v]);
                nurbs_derivs(*degree, knots, *count, ctrl, u, v)
            }
            EvalSurface::Revolved { profile, origin, axis, transform, swap } => {
                let [u, v] = self.wrap([u, v]);
                let (s, a) = if *swap { (v, u) } else { (u, v) };
                let (c, dc) = profile.eval(s)?;
                let (sin, cos) = a.sin_cos();
                let rot = |x: Vec3| x * cos + axis.cross(x) * sin + *axis * (axis.dot(x) * (1.0 - cos));
                let r = c - *origin;
                let p = *origin + rot(r);
                let ds = rot(dc);
                // d/dθ of the rotated point: axis × (rotated radius).
                let da = axis.cross(rot(r));
                let (p, ds, da) = (affine(transform, p), linear(transform, ds), linear(transform, da));
                Some(if *swap { (p, da, ds) } else { (p, ds, da) })
            }
        }
    }
}

impl EvalSurface {
    /// The second partial derivatives `(S_uu, S_uv, S_vv)` at (u, v) of a plane or spline (zero
    /// on a plane); `None` for a revolution (its profile carries first derivatives only) and
    /// where the spline cannot be evaluated.
    pub fn second(&self, u: f64, v: f64) -> Option<(Vec3, Vec3, Vec3)> {
        if !(u.is_finite() && v.is_finite()) {
            return None;
        }
        match self {
            EvalSurface::Plane { .. } => Some((Vec3::ZERO, Vec3::ZERO, Vec3::ZERO)),
            EvalSurface::Nurbs { degree, knots, count, ctrl, .. } => {
                let [u, v] = self.wrap([u, v]);
                nurbs_second(*degree, knots, *count, ctrl, u, v)
            }
            EvalSurface::Revolved { .. } => None,
        }
    }
}

/// The values and first two derivatives of the `p + 1` basis functions of degree `p` alive at
/// `t` ([`crate::bspline::basis_derivatives`]), and the span they live on. A `t` past an end of
/// the knot range by no more than rounding (a boundary point carried off by a pcurve's
/// evaluation, which [`nurbs_derivs`] takes as it is) is that end: the basis functions are zero
/// outside their supports, so the derivatives there would not exist.
fn basis_second(knots: &[f64], p: usize, n: usize, t: f64) -> Option<(usize, Vec<[f64; 3]>)> {
    let (first, ders) = crate::bspline::basis_derivatives(knots, p, n, onto_knots(knots, p, n, t)?, 2)?;
    Some((first + p, ders.iter().map(|d| Some([*d.first()?, *d.get(1)?, *d.get(2)?])).collect::<Option<_>>()?))
}

/// `t`, or the end of the knot range of a spline of degree `p` over `n` points when `t` is past
/// it by no more than rounding ([`basis_second`]).
pub(crate) fn onto_knots(knots: &[f64], p: usize, n: usize, t: f64) -> Option<f64> {
    let (&lo, &hi) = (knots.get(p)?, knots.get(n)?);
    let slack = BOUNDARY_ROUNDING * (hi - lo).abs().max(lo.abs()).max(hi.abs()).max(1.0);
    Some(if t < lo && t >= lo - slack {
        lo
    } else if t > hi && t <= hi + slack {
        hi
    } else {
        t
    })
}

/// How far past its knot range, relative to the range's magnitude, a parameter is still the range's
/// end: the room a pcurve's interval has for rounding (`brep::topology::PCURVE_INTERVAL`).
const BOUNDARY_ROUNDING: f64 = 1e-9;

fn nurbs_second(degree: [usize; 2], knots: &[Vec<f64>; 2], count: [usize; 2], ctrl: &[[f64; 4]], u: f64, v: f64) -> Option<(Vec3, Vec3, Vec3)> {
    let [pu, pv] = degree;
    let [nu, nv] = count;
    let ok = |p: usize, n: usize, k: &[f64]| (1..=MAX_SPLINE_DEGREE).contains(&p) && n > p && k.len() == n + p + 1;
    if !ok(pu, nu, &knots[0]) || !ok(pv, nv, &knots[1]) || nu.checked_mul(nv)? != ctrl.len() {
        return None;
    }
    let ((su, bu), (sv, bv)) = (basis_second(&knots[0], pu, nu, u)?, basis_second(&knots[1], pv, nv, v)?);
    // Homogeneous sums: [A, A_u, A_v, A_uu, A_uv, A_vv].
    let mut a = [[0.0f64; 4]; 6];
    for (j, bj) in bv.iter().enumerate() {
        for (i, bi) in bu.iter().enumerate() {
            let c = ctrl.get((sv + j - pv) * nu + su + i - pu)?;
            let w = [bi[0] * bj[0], bi[1] * bj[0], bi[0] * bj[1], bi[2] * bj[0], bi[1] * bj[1], bi[0] * bj[2]];
            for (sum, w) in a.iter_mut().zip(w) {
                for d in 0..4 {
                    sum[d] += w * c[d];
                }
            }
        }
    }
    let w = a[0][3];
    if !(w.abs() > 1e-300) {
        return None;
    }
    let xyz = |h: &[f64; 4]| Vec3::new(h[0], h[1], h[2]);
    // S = A / w, differentiated by the quotient rule.
    let s = xyz(&a[0]) * (1.0 / w);
    let su_ = (xyz(&a[1]) - s * a[1][3]) * (1.0 / w);
    let sv_ = (xyz(&a[2]) - s * a[2][3]) * (1.0 / w);
    let suu = (xyz(&a[3]) - su_ * (2.0 * a[1][3]) - s * a[3][3]) * (1.0 / w);
    let suv = (xyz(&a[4]) - su_ * a[2][3] - sv_ * a[1][3] - s * a[4][3]) * (1.0 / w);
    let svv = (xyz(&a[5]) - sv_ * (2.0 * a[2][3]) - s * a[5][3]) * (1.0 / w);
    (suu.is_finite() && suv.is_finite() && svv.is_finite()).then_some((suu, suv, svv))
}

fn affine(m: &[[f64; 4]; 3], p: Vec3) -> Vec3 {
    linear(m, p) + Vec3::new(m[0][3], m[1][3], m[2][3])
}

fn linear(m: &[[f64; 4]; 3], v: Vec3) -> Vec3 {
    let row = |r: &[f64; 4]| r[0] * v.x + r[1] * v.y + r[2] * v.z;
    Vec3::new(row(&m[0]), row(&m[1]), row(&m[2]))
}

fn det3(m: &[[f64; 4]; 3]) -> f64 {
    m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
}

/// The point and derivative of a rational function from its homogeneous value and derivative.
fn rational(a: [f64; 4], da: [f64; 4]) -> Option<(Vec3, Vec3)> {
    let w = a[3];
    if !(w.abs() > 1e-300) {
        return None;
    }
    let p = Vec3::new(a[0] / w, a[1] / w, a[2] / w);
    let d = (Vec3::new(da[0], da[1], da[2]) - p * da[3]) * (1.0 / w);
    (p.is_finite() && d.is_finite()).then_some((p, d))
}

fn nurbs_derivs(degree: [usize; 2], knots: &[Vec<f64>; 2], count: [usize; 2], ctrl: &[[f64; 4]], u: f64, v: f64) -> Option<(Vec3, Vec3, Vec3)> {
    let [pu, pv] = degree;
    let [nu, nv] = count;
    let ok = |p: usize, n: usize, k: &[f64]| (1..=MAX_SPLINE_DEGREE).contains(&p) && n > p && k.len() == n + p + 1;
    if !ok(pu, nu, &knots[0]) || !ok(pv, nv, &knots[1]) || nu.checked_mul(nv)? != ctrl.len() {
        return None;
    }
    let (su, sv) = (span_of(&knots[0], pu, nu, u), span_of(&knots[1], pv, nv, v));
    let (bu, bv) = (basis_ders(&knots[0], su, pu, u)?, basis_ders(&knots[1], sv, pv, v)?);
    let (mut s, mut s_u, mut s_v) = ([0.0; 4], [0.0; 4], [0.0; 4]);
    for (b, (nv0, nv1)) in bv[0].iter().zip(&bv[1]).take(pv + 1).enumerate() {
        for (a, (nu0, nu1)) in bu[0].iter().zip(&bu[1]).take(pu + 1).enumerate() {
            let c = ctrl.get((sv + b - pv) * nu + su + a - pu)?;
            for d in 0..4 {
                s[d] += nu0 * nv0 * c[d];
                s_u[d] += nu1 * nv0 * c[d];
                s_v[d] += nu0 * nv1 * c[d];
            }
        }
    }
    let (p, du) = rational(s, s_u)?;
    let (_, dv) = rational(s, s_v)?;
    Some((p, du, dv))
}

/// The knot span used at `t`: the last non-empty span starting at or before `t`, clamped to
/// the valid spans `p ≤ k < n` so parameters just outside the domain extend the end spans.
pub(crate) fn span_of(knots: &[f64], p: usize, n: usize, t: f64) -> usize {
    let mut k = p;
    for i in p..n {
        if let (Some(&a), Some(&b)) = (knots.get(i), knots.get(i + 1))
            && a < b
        {
            if a <= t {
                k = i;
            } else {
                break;
            }
        }
    }
    k
}

/// Non-zero basis functions of degree `p` on span `span` and their first derivatives
/// (NURBS Book A2.3 with n = 1). `ders[0][j]` is `N_{span−p+j}`, `ders[1][j]` its derivative.
pub(crate) fn basis_ders(knots: &[f64], span: usize, p: usize, t: f64) -> Option<[[f64; W]; 2]> {
    if p == 0 || p >= W {
        return None;
    }
    let kn = |i: usize| knots.get(i).copied();
    // ndu[j][r]: basis values (upper triangle incl. diagonal), knot differences (lower).
    let mut ndu = [[0.0f64; W]; W];
    let (mut left, mut right) = ([0.0f64; W], [0.0f64; W]);
    ndu[0][0] = 1.0;
    for j in 1..=p {
        left[j] = t - kn((span + 1).checked_sub(j)?)?;
        right[j] = kn(span + j)? - t;
        let mut saved = 0.0;
        for r in 0..j {
            ndu[j][r] = right[r + 1] + left[j - r];
            let temp = if ndu[j][r] != 0.0 { ndu[r][j - 1] / ndu[j][r] } else { 0.0 };
            ndu[r][j] = saved + right[r + 1] * temp;
            saved = left[j - r] * temp;
        }
        ndu[j][j] = saved;
    }
    let mut ders = [[0.0f64; W]; 2];
    for j in 0..=p {
        ders[0][j] = ndu[j][p];
    }
    // First derivative: p·(N_{r,p−1}/(u_{r+p}−u_r) − N_{r+1,p−1}/(u_{r+p+1}−u_{r+1})).
    for r in 0..=p {
        let mut d = 0.0;
        if r >= 1 {
            let den = ndu[p][r - 1];
            if den != 0.0 {
                d += ndu[r - 1][p - 1] / den;
            }
        }
        if r < p {
            let den = ndu[p][r];
            if den != 0.0 {
                d -= ndu[r][p - 1] / den;
            }
        }
        ders[1][r] = d * p as f64;
    }
    Some(ders)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Vec3, b: Vec3, tol: f64) -> bool {
        a.dist(b) <= tol
    }

    /// A moved plan evaluates to the plan's points moved, on a rational spline surface (the
    /// translation scales with each weight) and a revolved one (composed with its own map).
    #[test]
    fn a_transformed_plan_evaluates_to_the_moved_points() {
        let (c, s) = (0.6f64.cos(), 0.6f64.sin());
        // Turned about z by 0.6 rad, then moved: by columns.
        let m = [[c, s, 0.0, 0.0], [-s, c, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [5.0, -2.0, 7.0, 1.0]];
        let apply = |p: Vec3| Vec3::new(c * p.x - s * p.y + 5.0, s * p.x + c * p.y - 2.0, p.z + 7.0);
        let w = std::f64::consts::FRAC_1_SQRT_2;
        let nurbs = EvalSurface::nurbs(
            [2, 1],
            [vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0], vec![0.0, 0.0, 1.0, 1.0]],
            [3, 2],
            vec![[1.0, 0.0, 0.0, 1.0], [w, w, 0.0, w], [0.0, 1.0, 0.0, 1.0], [1.0, 0.0, 2.0, 1.0], [w, w, 2.0 * w, w], [0.0, 1.0, 2.0, 1.0]],
        );
        let revolved = EvalSurface::Revolved {
            profile: line(Vec3::new(2.0, 0.0, 0.0), Vec3::new(3.0, 0.0, 1.0)),
            origin: Vec3::ZERO,
            axis: Vec3::Z,
            transform: [[1.0, 0.0, 0.0, 1.0], [0.0, 0.0, -1.0, 2.0], [0.0, 1.0, 0.0, 3.0]],
            swap: false,
        };
        let face = |i: u32| FacePlan {
            face: i,
            surface: Some(i),
            uv: vec![[0.1, 0.2], [0.7, 0.4], [0.3, 0.9]],
            fixed: vec![Vec3::new(1.0, 2.0, 3.0)],
            triangles: vec![[0, 1, 2]],
            ..Default::default()
        };
        let plan = TessPlan {
            surfaces: vec![nurbs, revolved],
            faces: vec![face(0), face(1)],
            edges: vec![vec![Vec3::new(1.0, 1.0, 1.0)]],
            edge_faces: vec![vec![0, 1]],
        };
        let (a, b) = (plan.evaluate(), plan.transformed(&m).evaluate());
        assert_eq!(a.triangles, b.triangles);
        for (p, q) in a.positions.iter().zip(&b.positions) {
            assert!(close(apply(*p), *q, 1e-12), "{p:?} -> {q:?}");
        }
        for (n, k) in a.normals.iter().zip(&b.normals) {
            assert!(close(apply(*n) - apply(Vec3::ZERO), *k, 1e-12));
        }
        assert!(close(apply(a.edges[0][0]), b.edges[0][0], 1e-12));
    }

    fn line(a: Vec3, b: Vec3) -> EvalCurve {
        EvalCurve { degree: 1, knots: vec![0.0, 0.0, 1.0, 1.0], ctrl: vec![[a.x, a.y, a.z, 1.0], [b.x, b.y, b.z, 1.0]] }
    }

    const IDENTITY: [[f64; 4]; 3] = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]];

    #[test]
    fn plane_point_and_normal() {
        let s = EvalSurface::Plane { origin: Vec3::new(1.0, 2.0, 3.0), du: Vec3::X, dv: Vec3::Y };
        let (p, n) = s.eval(2.0, 3.0);
        assert!(close(p, Vec3::new(3.0, 5.0, 3.0), 1e-12));
        assert!(close(n, Vec3::Z, 1e-12));
    }

    #[test]
    fn revolved_line_is_a_cylinder() {
        // A vertical line at radius 2 turned about Z.
        let s = EvalSurface::Revolved {
            profile: line(Vec3::new(2.0, 0.0, 0.0), Vec3::new(2.0, 0.0, 5.0)),
            origin: Vec3::ZERO,
            axis: Vec3::Z,
            transform: IDENTITY,
            swap: false,
        };
        for &(u, v) in &[(0.0, 0.0), (0.5, 1.0), (1.0, 4.0), (0.25, 6.0)] {
            let (p, n) = s.eval(u, v);
            assert!(close(p, Vec3::new(2.0 * v.cos(), 2.0 * v.sin(), 5.0 * u), 1e-12), "{p:?}");
            // S_u = +Z, S_v = tangential: the normal points inward (−radial) for this profile.
            assert!(close(n, Vec3::new(-v.cos(), -v.sin(), 0.0), 1e-12), "{n:?}");
        }
    }

    #[test]
    fn swapped_parameters_reverse_the_normal() {
        let mk = |swap| EvalSurface::Revolved {
            profile: line(Vec3::new(2.0, 0.0, 0.0), Vec3::new(2.0, 0.0, 5.0)),
            origin: Vec3::ZERO,
            axis: Vec3::Z,
            transform: IDENTITY,
            swap,
        };
        let (p0, n0) = mk(false).eval(0.3, 1.1);
        let (p1, n1) = mk(true).eval(1.1, 0.3);
        assert!(close(p0, p1, 1e-12));
        assert!(close(n0, -n1, 1e-12));
    }

    #[test]
    fn mirror_transform_keeps_the_normal_outward() {
        let mirror = [[-1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]];
        let s = EvalSurface::Revolved {
            profile: line(Vec3::new(2.0, 0.0, 0.0), Vec3::new(2.0, 0.0, 5.0)),
            origin: Vec3::ZERO,
            axis: Vec3::Z,
            transform: mirror,
            swap: false,
        };
        let (p, n) = s.eval(0.5, 0.0);
        assert!(close(p, Vec3::new(-2.0, 0.0, 2.5), 1e-12));
        // The unmirrored normal at θ = 0 is −X; mirrored it is +X, still pointing to the axis.
        assert!(close(n, Vec3::X, 1e-12), "{n:?}");
    }

    #[test]
    fn rational_quarter_circle_profile_is_exact() {
        // A quarter circle of radius 3 in the XZ plane, turned about Z: a sphere zone.
        let w = std::f64::consts::FRAC_1_SQRT_2;
        let profile = EvalCurve {
            degree: 2,
            knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            ctrl: vec![[3.0, 0.0, 0.0, 1.0], [3.0 * w, 0.0, 3.0 * w, w], [0.0, 0.0, 3.0, 1.0]],
        };
        let s = EvalSurface::Revolved { profile, origin: Vec3::ZERO, axis: Vec3::Z, transform: IDENTITY, swap: false };
        for i in 0..=10 {
            for j in 0..=6 {
                let (u, v) = (i as f64 / 10.0, j as f64);
                let (p, n) = s.eval(u, v);
                assert!((p.len() - 3.0).abs() < 1e-12, "{p:?}");
                // S_u × S_v points into this sphere. At the pole (u = 1) the normal comes from
                // a step inside: within ~1e-6 rad, and the same way (a step past the pole would
                // turn it over).
                assert!(n.cross(p).len() < 3e-5 && n.dot(p) < 0.0, "normal {n:?} not inward at {p:?}");
            }
        }
    }

    #[test]
    fn bilinear_nurbs_patch_and_derivatives() {
        // Degree (1, 1) over [0, 2] × [0, 1] with corners lifted: a hyperbolic paraboloid.
        let ctrl = vec![[0.0, 0.0, 0.0, 1.0], [2.0, 0.0, 0.0, 1.0], [0.0, 1.0, 0.0, 1.0], [2.0, 1.0, 1.0, 1.0]];
        let s = EvalSurface::nurbs([1, 1], [vec![0.0, 0.0, 2.0, 2.0], vec![0.0, 0.0, 1.0, 1.0]], [2, 2], ctrl);
        let Some((p, du, dv)) = s.derivs(1.0, 0.5) else { panic!("evaluates") };
        assert!(close(p, Vec3::new(1.0, 0.5, 0.25), 1e-12), "{p:?}");
        assert!(close(du, Vec3::new(1.0, 0.0, 0.25), 1e-12), "{du:?}");
        assert!(close(dv, Vec3::new(0.0, 1.0, 0.5), 1e-12), "{dv:?}");
    }

    #[test]
    fn cubic_nurbs_matches_the_geom_surface() {
        // Cross-check the evaluator against `bspline::NurbsSurface::eval` on a weighted patch.
        let knots = [vec![0.0, 0.0, 0.0, 0.0, 0.4, 1.0, 1.0, 1.0, 1.0], vec![0.0, 0.0, 0.0, 1.0, 2.0, 2.0, 2.0]];
        let (nu, nv) = (5, 4);
        let mut pts = Vec::new();
        let mut wts = Vec::new();
        for j in 0..nv {
            for i in 0..nu {
                pts.push(Vec3::new(i as f64, j as f64, ((i * 7 + j * 3) % 5) as f64 * 0.3));
                wts.push(1.0 + ((i + 2 * j) % 3) as f64 * 0.25);
            }
        }
        let Ok(reference) = crate::bspline::NurbsSurface::new([3, 2], knots.clone(), &pts, &wts) else { panic!("valid surface") };
        let ctrl = reference.homogeneous().iter().map(|h| [h.x, h.y, h.z, h.w]).collect();
        let s = EvalSurface::nurbs([3, 2], knots, [nu, nv], ctrl);
        for i in 0..=8 {
            for j in 0..=8 {
                let (u, v) = (i as f64 / 8.0, j as f64 / 4.0);
                let Ok(want) = reference.eval(u, v) else { panic!("in domain") };
                let Some((p, du, dv)) = s.derivs(u, v) else { panic!("evaluates") };
                assert!(close(p, want, 1e-12), "({u}, {v}): {p:?} vs {want:?}");
                // Derivatives against central differences (one-sided at the domain ends).
                let h = 1e-6;
                let fd = |a: (f64, f64), b: (f64, f64)| {
                    let (Some(pa), Some(pb)) = (s.derivs(a.0, a.1), s.derivs(b.0, b.1)) else { panic!("evaluates") };
                    (pb.0 - pa.0) * (1.0 / (b.0 - a.0 + b.1 - a.1))
                };
                let fu = fd((u.max(h) - h, v), ((u + h).min(1.0), v));
                let fv = fd((u, v.max(h) - h), (u, (v + h).min(2.0)));
                assert!(close(du, fu, 1e-4 * (1.0 + du.len())), "du at ({u}, {v}): {du:?} vs {fu:?}");
                assert!(close(dv, fv, 1e-4 * (1.0 + dv.len())), "dv at ({u}, {v}): {dv:?} vs {fv:?}");
            }
        }
    }

    #[test]
    fn hostile_surfaces_do_not_panic() {
        let bad = [
            EvalSurface::nurbs([9, 1], [vec![0.0; 3], vec![0.0; 3]], [1, 1], vec![]),
            EvalSurface::nurbs([1, 1], [vec![0.0, 0.0, 1.0, 1.0], vec![0.0, 0.0, 1.0, 1.0]], [2, 2], vec![[0.0; 4]; 4]),
            EvalSurface::Revolved {
                profile: EvalCurve { degree: 0, knots: vec![], ctrl: vec![] },
                origin: Vec3::ZERO,
                axis: Vec3::Z,
                transform: IDENTITY,
                swap: false,
            },
        ];
        for s in &bad {
            for &(u, v) in &[(0.5, 0.5), (f64::NAN, 0.0), (1e300, -1e300)] {
                let (p, n) = s.eval(u, v);
                assert!(p.is_finite() && n.is_finite());
            }
        }
    }

    #[test]
    fn evaluate_uses_fixed_points_and_drops_bad_indices() {
        let plan = TessPlan {
            surfaces: vec![EvalSurface::Plane { origin: Vec3::ZERO, du: Vec3::X, dv: Vec3::Y }],
            faces: vec![FacePlan {
                face: 3,
                surface: Some(0),
                uv: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]],
                fixed: vec![Vec3::new(0.0, 0.0, 1e-9)],
                normals: vec![],
                flip: true,
                triangles: vec![[0, 1, 2], [0, 1, 7]],
            }],
            edges: vec![],
            edge_faces: vec![],
        };
        let m = plan.evaluate();
        assert_eq!(m.positions.len(), 3);
        assert_eq!(m.positions.first().copied(), Some(Vec3::new(0.0, 0.0, 1e-9)));
        assert_eq!(m.triangles, vec![[0, 1, 2]]);
        assert_eq!(m.tri_face, vec![3]);
        assert!(m.normals.iter().all(|n| close(*n, -Vec3::Z, 1e-12)));
        // Faces with triangles, counted without evaluating, are the faces the mesh has: not a
        // face whose only triangle is off its nodes, nor one with none, nor one out of range.
        let mut plan = plan;
        let only = |face: u32, triangles: Vec<[u32; 3]>| FacePlan { face, uv: vec![[0.0, 0.0]; 3], triangles, ..Default::default() };
        plan.faces.extend([only(0, vec![[0, 1, 9]]), only(1, vec![]), only(2, vec![[2, 1, 0]]), only(7, vec![[0, 1, 2]])]);
        let has = plan.faces_with_triangles(5);
        let mut evaluated = vec![false; 5];
        for f in plan.evaluate().tri_face {
            if let Some(h) = evaluated.get_mut(f as usize) {
                *h = true;
            }
        }
        assert_eq!(has, evaluated);
        assert_eq!(has, vec![false, false, true, true, false]);
    }
}
