//! The G^n section's stations: the faces' partial derivatives (a NURBS face by its own net,
//! continued past its knots), the blend's system at a station (the two feet on the faces at
//! distance `tau r_i` and the plane square to the leader), and the feet's truncated power
//! series in `(sigma, eps)` solved order by order with the station's Jacobian.
//!
//! Fork: `sm_Partials`, `sm_Nurbs`, `sm_NurbsOf`, `sm_Span`, `sm_DersBasis`,
//! `sm_NurbsPartials`, `sm_SurfacePartials`, `sm_GnSystem`, `sm_GnEvaluate`,
//! `sm_GnJacobian`, `sm_Solve4`, `sm_GnStation`, `sm_LeaderSeries`, `sm_GnSolveStation` in
//! `source/SMLib/src/SmFilletSection.cpp` (tangency-labs/smlib@11ea2d9); see [`super`] for
//! the table.

use std::borrow::Cow;

use super::series::{GnScalar, Ser, Trunc, VSer, MAX_K, MAX_S, V3};
use crate::error::{Result, SolidError};
use crate::kernel::Kernel;
use crate::math::{Point2, Vec3};
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::b_spline_surface::p1::SurfParamType;
use crate::nurbs::b_spline_surface::BSplineSurface;
use crate::nurbs::b_spline_surface_n_lib::p2::EvaluateFlags;
use crate::nurbs::math::NOW;
use crate::surface::surface::{Surface, SurfaceKind};

const OP: &str = "blend::fillet_section::station";

/// `smos_Max(a, b)`: `(a > b) ? a : b`.
#[inline]
pub(crate) fn smax(a: f64, b: f64) -> f64 {
    if a > b {
        a
    } else {
        b
    }
}

/// A row-major matrix read and written by signed indices, as the fork's nested vectors
/// are; an index outside it reads 0 and is not written (the fork's indices stay inside).
#[derive(Clone, Debug)]
pub(crate) struct Mat<T> {
    cols: usize,
    data: Vec<T>,
}

impl<T: GnScalar> Mat<T> {
    pub(crate) fn zeros(rows: usize, cols: usize) -> Self {
        Self {
            cols,
            data: vec![T::of(0.0); rows * cols],
        }
    }

    fn index(&self, i: isize, j: isize) -> Option<usize> {
        let i = usize::try_from(i).ok()?;
        let j = usize::try_from(j).ok()?;
        if j >= self.cols {
            return None;
        }
        Some(i * self.cols + j)
    }

    pub(crate) fn at(&self, i: isize, j: isize) -> T {
        self.index(i, j)
            .and_then(|x| self.data.get(x))
            .copied()
            .unwrap_or_else(|| T::of(0.0))
    }

    pub(crate) fn set(&mut self, i: isize, j: isize, v: T) {
        if let Some(x) = self.index(i, j).and_then(|x| self.data.get_mut(x)) {
            *x = v;
        }
    }
}

/// `crU[i]` of a knot vector by a signed index (NaN outside it: never, for the fork's
/// indices, and a NaN shows in the result if it did).
#[inline]
pub(crate) fn knot(u: &[f64], i: isize) -> f64 {
    usize::try_from(i)
        .ok()
        .and_then(|i| u.get(i))
        .copied()
        .unwrap_or(f64::NAN)
}

/// `sm_Partials`: a face's partial derivatives at a point, `P[a][b] = d^(a+b) S / du^a dv^b`
/// for `a + b <=` the order asked for (`a, b <= MAX_K`).
#[derive(Clone, Debug, PartialEq)]
pub struct Partials<T> {
    p: Vec<V3<T>>,
}

impl<T: GnScalar> Default for Partials<T> {
    fn default() -> Self {
        let z = V3 {
            x: T::of(0.0),
            y: T::of(0.0),
            z: T::of(0.0),
        };
        Self {
            p: vec![z; (MAX_K + 1) * (MAX_K + 1)],
        }
    }
}

impl<T: GnScalar> Partials<T> {
    /// `P[a][b]` (0 past `MAX_K`).
    #[must_use]
    pub fn at(&self, a: usize, b: usize) -> V3<T> {
        let z = V3 {
            x: T::of(0.0),
            y: T::of(0.0),
            z: T::of(0.0),
        };
        if a > MAX_K || b > MAX_K {
            return z;
        }
        self.p.get(a * (MAX_K + 1) + b).copied().unwrap_or(z)
    }

    /// Sets `P[a][b]` (ignored past `MAX_K`).
    pub fn set(&mut self, a: usize, b: usize, v: V3<T>) {
        if a > MAX_K || b > MAX_K {
            return;
        }
        if let Some(x) = self.p.get_mut(a * (MAX_K + 1) + b) {
            *x = v;
        }
    }
}

/// `sm_Nurbs`: a NURBS surface's data, for its partials at any `(u, v)`: a point outside the
/// knots' range is on the analytic continuation of the end span (a plane goes on as the
/// plane, a cylinder's arc as its circle), the face's own surface beyond its trimmed domain.
#[derive(Clone, Debug, PartialEq)]
pub struct Nurbs {
    /// Degree in u.
    pub p: usize,
    /// Degree in v.
    pub q: usize,
    /// Control points in u.
    pub nu: usize,
    /// Control points in v.
    pub nv: usize,
    /// The full u knot vector.
    pub u: Vec<f64>,
    /// The full v knot vector.
    pub v: Vec<f64>,
    /// The homogeneous control points `(x w, y w, z w, w)`, u-major.
    pub pw: Vec<f64>,
}

/// The full knot vector of unique knots with multiplicities (`GetKnotsPointers`' storage).
fn full_knots(knots: &[f64], mult: &[usize]) -> Vec<f64> {
    let mut u = Vec::new();
    for (&k, &m) in knots.iter().zip(mult) {
        for _ in 0..m {
            u.push(k);
        }
    }
    u
}

impl Nurbs {
    /// `sm_NurbsOf`: the data of a B-spline surface.
    ///
    /// # Errors
    ///
    /// `SM_ERR` when the surface has no NURB, its degrees, knots and net do not agree, or a
    /// weight is not positive.
    pub fn of(s: &BSplineSurface) -> Result<Self> {
        let (ku, mu) = s.knots(SurfParamType::U, None)?;
        let (kv, mv) = s.knots(SurfParamType::V, None)?;
        let (lnu, lnv, cps) = s.control_points()?;
        let p = s.degree(SurfParamType::U);
        let q = s.degree(SurfParamType::V);
        let u = full_knots(&ku, &mu);
        let v = full_knots(&kv, &mv);
        if p < 1 || q < 1 || u.len() != lnu + p + 1 || v.len() != lnv + q + 1 {
            // "SmGnSectionFSG: a face's NURBS data do not agree"
            return Err(SolidError::sm_err(OP));
        }
        if cps.len() != lnu * lnv {
            return Err(SolidError::sm_err(OP));
        }
        let mut pw = Vec::new();
        for c in &cps {
            let rational = c.w != NOW;
            let w = if rational { c.w } else { 1.0 };
            pw.extend_from_slice(&[c.x, c.y, c.z, w]);
            let positive = w > 0.0;
            if !positive {
                // "SmGnSectionFSG: a face's weights are not positive"
                return Err(SolidError::sm_err(OP));
            }
        }
        Ok(Self {
            p,
            q,
            nu: lnu,
            nv: lnv,
            u,
            v,
            pw,
        })
    }

    fn pw_at(&self, i: usize) -> f64 {
        self.pw.get(i).copied().unwrap_or(f64::NAN)
    }
}

/// `sm_Span`: the span k (`U[k] <= u < U[k+1]`, `p <= k < n`) of a parameter, the end spans
/// continued past the knots' range.
#[must_use]
pub fn span(u: &[f64], p: usize, n: usize, x: f64) -> usize {
    let mut k = p;
    while k + 1 < n && x >= knot(u, k as isize + 1) {
        k += 1;
    }
    while k > p && knot(u, k as isize) >= knot(u, k as isize + 1) {
        k -= 1;
    }
    k
}

/// `sm_DersBasis`: the non-zero basis functions `N_{k-p..k}` and their derivatives up to
/// `d` at `x` in span `k`, `ders[d][j]` (the NURBS book's A2.3, its knot differences taken
/// from the knots directly).
#[must_use]
pub fn ders_basis<T: GnScalar>(u: &[f64], k: usize, p: usize, x: T, d: usize) -> Vec<Vec<T>> {
    let (k, p) = (k as isize, p as isize);
    let pu = p as usize;
    let mut ndu = Mat::zeros(pu + 1, pu + 1);
    let mut left = vec![T::of(0.0); pu + 1];
    let mut right = vec![T::of(0.0); pu + 1];
    let at = |v: &[T], i: isize| {
        usize::try_from(i)
            .ok()
            .and_then(|i| v.get(i))
            .copied()
            .unwrap_or_else(|| T::of(0.0))
    };
    let put = |v: &mut [T], i: isize, x: T| {
        if let Some(y) = usize::try_from(i).ok().and_then(|i| v.get_mut(i)) {
            *y = x;
        }
    };
    ndu.set(0, 0, T::of(1.0));
    for j in 1..=p {
        put(&mut left, j, x - T::of(knot(u, k + 1 - j)));
        put(&mut right, j, T::of(knot(u, k + j)) - x);
        let mut saved = T::of(0.0);
        for r in 0..j {
            ndu.set(j, r, T::of(knot(u, k + r + 1) - knot(u, k + 1 - j + r)));
            let temp = ndu.at(r, j - 1) / ndu.at(j, r);
            ndu.set(r, j, saved + at(&right, r + 1) * temp);
            saved = at(&left, j - r) * temp;
        }
        ndu.set(j, j, saved);
    }
    let mut ders = Mat::zeros(d + 1, pu + 1);
    for j in 0..=p {
        ders.set(0, j, ndu.at(j, p));
    }
    let mut a = Mat::zeros(2, pu + 1);
    let du = if (d as isize) < p { d as isize } else { p };
    for r in 0..=p {
        let (mut s1, mut s2) = (0_isize, 1_isize);
        a.set(0, 0, T::of(1.0));
        for kk in 1..=du {
            let mut dd = T::of(0.0);
            let rk = r - kk;
            let pk = p - kk;
            if r >= kk {
                a.set(s2, 0, a.at(s1, 0) / ndu.at(pk + 1, rk));
                dd = a.at(s2, 0) * ndu.at(rk, pk);
            }
            let j1 = if rk >= -1 { 1 } else { -rk };
            let j2 = if r - 1 <= pk { kk - 1 } else { p - r };
            for j in j1..=j2 {
                a.set(
                    s2,
                    j,
                    (a.at(s1, j) - a.at(s1, j - 1)) / ndu.at(pk + 1, rk + j),
                );
                dd = dd + a.at(s2, j) * ndu.at(rk + j, pk);
            }
            if r <= pk {
                a.set(s2, kk, (T::of(0.0) - a.at(s1, kk - 1)) / ndu.at(pk + 1, r));
                dd = dd + a.at(s2, kk) * ndu.at(r, pk);
            }
            ders.set(kk, r, dd);
            core::mem::swap(&mut s1, &mut s2);
        }
    }
    let mut f = p as f64;
    for kk in 1..=du {
        for j in 0..=p {
            ders.set(kk, j, ders.at(kk, j) * T::of(f));
        }
        f *= (p - kk) as f64;
    }
    (0..=d as isize)
        .map(|i| (0..=p).map(|j| ders.at(i, j)).collect())
        .collect()
}

/// `sm_NurbsPartials`: the partials `P[a][b]`, `a + b <= lk`, of the NURBS at `(u, v)` in
/// the spans `(ku, kv)` (the NURBS book's A4.4).
#[must_use]
pub fn nurbs_partials<T: GnScalar>(
    n: &Nurbs,
    ku: usize,
    kv: usize,
    u: T,
    v: T,
    lk: usize,
) -> Partials<T> {
    let mut out = Partials::default();
    let nu = ders_basis(&n.u, ku, n.p, u, lk);
    let nv = ders_basis(&n.v, kv, n.q, v, lk);
    let get = |m: &[Vec<T>], i: usize, j: usize| {
        m.get(i)
            .and_then(|r| r.get(j))
            .copied()
            .unwrap_or_else(|| T::of(0.0))
    };
    // the homogeneous partials A (x, y, z) and w
    let mut ax = Mat::zeros(lk + 1, lk + 1);
    let mut ay = Mat::zeros(lk + 1, lk + 1);
    let mut az = Mat::zeros(lk + 1, lk + 1);
    let mut w = Mat::zeros(lk + 1, lk + 1);
    for a in 0..=lk {
        for b in 0..=(lk - a) {
            let (mut sx, mut sy, mut sz, mut sw) = (T::of(0.0), T::of(0.0), T::of(0.0), T::of(0.0));
            for i in 0..=n.p {
                for j in 0..=n.q {
                    // ku >= p and kv >= q (sm_Span)
                    let li = 4 * ((ku + i - n.p) * n.nv + (kv + j - n.q));
                    let c = get(&nu, a, i) * get(&nv, b, j);
                    sx = sx + c * T::of(n.pw_at(li));
                    sy = sy + c * T::of(n.pw_at(li + 1));
                    sz = sz + c * T::of(n.pw_at(li + 2));
                    sw = sw + c * T::of(n.pw_at(li + 3));
                }
            }
            let (ia, ib) = (a as isize, b as isize);
            ax.set(ia, ib, sx);
            ay.set(ia, ib, sy);
            az.set(ia, ib, sz);
            w.set(ia, ib, sw);
        }
    }
    for a in 0..=lk {
        for b in 0..=(lk - a) {
            let (ia, ib) = (a as isize, b as isize);
            let mut v3 = V3 {
                x: ax.at(ia, ib),
                y: ay.at(ia, ib),
                z: az.at(ia, ib),
            };
            let mut cka = 1.0_f64;
            for i in 0..=a {
                if i > 0 {
                    cka = cka * (a - i + 1) as f64 / i as f64;
                }
                let mut cbj = 1.0_f64;
                for j in 0..=b {
                    if j > 0 {
                        cbj = cbj * (b - j + 1) as f64 / j as f64;
                    }
                    if i == 0 && j == 0 {
                        continue;
                    }
                    let c = T::of(cka * cbj) * w.at(i as isize, j as isize);
                    let pij = out.at(a - i, b - j);
                    v3.x = v3.x - c * pij.x;
                    v3.y = v3.y - c * pij.y;
                    v3.z = v3.z - c * pij.z;
                }
            }
            let w00 = w.at(0, 0);
            out.set(
                a,
                b,
                V3 {
                    x: v3.x / w00,
                    y: v3.y / w00,
                    z: v3.z / w00,
                },
            );
        }
    }
    out
}

/// The `SmBSplineSurface` a surface is (an analytic surface is one: a plane, a cone or
/// cylinder, a sphere, a torus, a surface of revolution or of extrusion), with its NURB;
/// `None` for any other kind (`IsKindOf(SmBSplineSurface_TYPE)` false).
///
/// # Errors
///
/// Those of making the NURB.
pub fn as_b_spline<'a>(k: &Kernel, s: &'a Surface) -> Result<Option<Cow<'a, BSplineSurface>>> {
    Ok(match s.kind_with_nurb(k)? {
        SurfaceKind::BSpline(b) => Some(Cow::Borrowed(b)),
        SurfaceKind::Plane(p) => Some(p.base_with_nurb()?),
        SurfaceKind::Cone(c) => Some(Cow::Borrowed(c.rev().base())),
        SurfaceKind::SurfOfRevolution(r) => Some(Cow::Borrowed(r.base())),
        SurfaceKind::Sphere(sp) => Some(Cow::Borrowed(sp.base().base())),
        SurfaceKind::Torus(t) => Some(Cow::Borrowed(t.base().base())),
        SurfaceKind::SurfOfExtrusion(e) => Some(e.base_with_nurb(k)?),
        SurfaceKind::CurveBounded(_)
        | SurfaceKind::Step(_)
        | SurfaceKind::OffsetSurface(_)
        | SurfaceKind::SrfInVolume(_) => None,
    })
}

/// `sm_SurfacePartials`: the partials of a face's surface at `uv` up to total order `lk`:
/// a NURBS (every analytic surface is one, evaluated in its net's parameters) by its net,
/// continued past its knots; any other surface by its own evaluator.
///
/// # Errors
///
/// `SM_ERR` for `lk > MAX_K`; those of [`Nurbs::of`] and the surface's evaluator.
pub fn surface_partials(k: &Kernel, s: &Surface, uv: Point2, lk: usize) -> Result<Partials<f64>> {
    if lk > MAX_K {
        return Err(SolidError::sm_err(OP));
    }
    if let Some(bs) = as_b_spline(k, s)? {
        let n = Nurbs::of(&bs)?;
        let ku = span(&n.u, n.p, n.nu, uv.x);
        let kv = span(&n.v, n.q, n.nv, uv.y);
        return Ok(nurbs_partials(&n, ku, kv, uv.x, uv.y, lk));
    }
    // Evaluate(crUV, lK, lK, TRUE, TRUE, TRUE, sD, FALSE)
    let flags = EvaluateFlags {
        only_upper_half: true,
        non_zero_tangents: false,
        ..EvaluateFlags::default()
    };
    let d = s.evaluate(k, uv, lk, lk, flags)?;
    let mut out = Partials::default();
    for a in 0..=lk {
        for b in 0..=(lk - a) {
            let v = d
                .get(a)
                .and_then(|r| r.get(b))
                .copied()
                .ok_or_else(|| SolidError::sm_err(OP))?;
            out.set(a, b, V3::from_vec3(v));
        }
    }
    Ok(out)
}

/// `sm_GnSystem`: the blend's system at a station: the two faces' partials at the feet, the
/// leader's point and unit tangent as series in sigma, the offsets `r_i` and the sides
/// `s_i`. `hold`: the offsets held at `tau = 1` (no eps in them).
#[derive(Clone, Debug)]
pub struct GnSystem<'a, T> {
    /// The faces' partials at the feet.
    pub p: [&'a Partials<T>; 2],
    /// The leader's point.
    pub leader: VSer<T>,
    /// The leader's unit tangent.
    pub tangent: VSer<T>,
    /// The offsets.
    pub r: [f64; 2],
    /// The sides.
    pub side: [f64; 2],
    /// The offsets held at `tau = 1`.
    pub hold: bool,
}

/// G's four series and the two feet's (`sm_GnEvaluate`'s `rG`, `rF`).
pub type GnValues<T> = ([Ser<T>; 4], [VSer<T>; 2]);

/// `sm_GnEvaluate`: G and the feet `F_i` as series, given the feet's parameters as series
/// `x[0..3]` (u1, v1, u2, v2), whose constant terms are the point the partials were taken
/// at.
///
/// # Errors
///
/// `SM_ERR` where a face's normal vanishes at a foot.
pub fn gn_evaluate<T: GnScalar>(
    sys: &GnSystem<'_, T>,
    x: &[Ser<T>; 4],
    tr: Trunc,
) -> Result<GnValues<T>> {
    let zero = V3 {
        x: T::of(0.0),
        y: T::of(0.0),
        z: T::of(0.0),
    };
    let lm = tr.s() + tr.e();
    let mut v_out = [VSer::constant(zero), VSer::constant(zero)];
    let mut f_out = [VSer::constant(zero), VSer::constant(zero)];
    let rows = x.chunks_exact(2).zip(sys.p).zip(sys.r.iter().zip(sys.side));
    for (f, ((xf, pf), (&rf, sidef))) in rows.enumerate() {
        let (Some(xu), Some(xv)) = (xf.first(), xf.get(1)) else {
            return Err(SolidError::sm_err(OP));
        };
        let mut du = *xu;
        let mut dv = *xv;
        du.set(0, 0, T::of(0.0));
        dv.set(0, 0, T::of(0.0));
        let mut pu = vec![Ser::constant(T::of(1.0))];
        let mut pv = vec![Ser::constant(T::of(1.0))];
        for _ in 1..=lm {
            let (Some(lu), Some(lv)) = (pu.last().copied(), pv.last().copied()) else {
                return Err(SolidError::sm_err(OP));
            };
            pu.push(lu.mul(&du, tr));
            pv.push(lv.mul(&dv, tr));
        }
        let mut s = VSer::constant(zero);
        let mut su = s;
        let mut sv = s;
        let mut fa = 1.0_f64;
        for (a, pua) in pu.iter().enumerate() {
            if a > 0 {
                fa *= a as f64;
            }
            let mut fb = 1.0_f64;
            for (b, pvb) in pv.iter().enumerate().take(lm - a + 1) {
                if b > 0 {
                    fb *= b as f64;
                }
                let mono = pua.mul(pvb, tr);
                let sf = T::of(1.0 / (fa * fb));
                let cs = [pf.at(a, b), pf.at(a + 1, b), pf.at(a, b + 1)];
                let outs: [&mut VSer<T>; 3] = [&mut s, &mut su, &mut sv];
                for (c, out) in cs.iter().zip(outs) {
                    let cv = [c.x, c.y, c.z];
                    for (o, &cj) in out.a.iter_mut().zip(&cv) {
                        *o = o.add(&mono.scale(cj * sf, tr), tr);
                    }
                }
            }
        }
        let nrm = su.cross(&sv, tr);
        let inv_len = nrm.dot(&nrm, tr).inv_sqrt(tr)?;
        // the offset towards the ball: tau r_i s_i, tau = 1 + eps
        let mut d = Ser::constant(T::of(rf * sidef));
        if !sys.hold && tr.e() >= 1 {
            d.set(0, 1, T::of(rf * sidef));
        }
        let vf = s.add(&nrm.times(&inv_len.mul(&d, tr), tr), tr);
        if let (Some(vo), Some(fo)) = (v_out.get_mut(f), f_out.get_mut(f)) {
            *vo = vf;
            *fo = s;
        }
    }
    let [v0, v1] = &v_out;
    let diff = v0.sub(v1, tr);
    let [g0, g1, g2] = diff.a;
    let g3 = v0.sub(&sys.leader, tr).dot(&sys.tangent, tr);
    Ok(([g0, g1, g2, g3], f_out))
}

/// `sm_GnJacobian`: the Jacobian `dG/dx` at x's constant terms, offsets held: G along
/// `x + e_j eps`, first order.
///
/// # Errors
///
/// Those of [`gn_evaluate`].
pub fn gn_jacobian<T: GnScalar>(sys: &GnSystem<'_, T>, x: &[Ser<T>; 4]) -> Result<[[T; 4]; 4]> {
    let tr = Trunc::new(0, 1)?;
    let mut held = sys.clone();
    held.hold = true;
    let mut jac = [[T::of(0.0); 4]; 4];
    for j in 0..4 {
        let mut sx = [Ser::constant(T::of(0.0)); 4];
        for (q, (sq, xq)) in sx.iter_mut().zip(x).enumerate() {
            *sq = Ser::constant(xq.at(0, 0));
            if q == j {
                sq.set(0, 1, T::of(1.0));
            }
        }
        let (g, _) = gn_evaluate(&held, &sx, tr)?;
        for (row, gi) in jac.iter_mut().zip(&g) {
            if let Some(x) = row.get_mut(j) {
                *x = gi.at(0, 1);
            }
        }
    }
    Ok(jac)
}

/// `sm_Solve4`: solves `A y = b`, 4 x 4, by Gaussian elimination with partial pivoting.
///
/// # Errors
///
/// `SM_ERR` where a pivot is at most `1e-13` of A's largest entry ("the blend's system is
/// singular at a station (its faces' normals agree there)").
pub fn solve4(ca: &[[f64; 4]; 4], cb: &[f64; 4]) -> Result<[f64; 4]> {
    // a[i][j], a[i][4] = b[i]
    let mut a = Mat::<f64>::zeros(4, 5);
    for (i, (ar, &b)) in ca.iter().zip(cb).enumerate() {
        for (j, &x) in ar.iter().enumerate() {
            a.set(i as isize, j as isize, x);
        }
        a.set(i as isize, 4, b);
    }
    let mut scale = 0.0_f64;
    for i in 0..4 {
        for j in 0..4 {
            scale = smax(scale, a.at(i, j).abs());
        }
    }
    for c in 0..4_isize {
        let mut p = c;
        for r in (c + 1)..4 {
            if a.at(r, c).abs() > a.at(p, c).abs() {
                p = r;
            }
        }
        let regular = a.at(p, c).abs() > 1.0e-13 * scale;
        if !regular {
            // "the blend's system is singular at a station (its faces' normals agree there)"
            return Err(SolidError::sm_err(OP));
        }
        for j in 0..5 {
            let t = a.at(c, j);
            a.set(c, j, a.at(p, j));
            a.set(p, j, t);
        }
        for r in (c + 1)..4 {
            let m = a.at(r, c) / a.at(c, c);
            for j in c..5 {
                a.set(r, j, a.at(r, j) - m * a.at(c, j));
            }
        }
    }
    let mut y = Mat::<f64>::zeros(1, 4);
    for c in (0..4_isize).rev() {
        let mut v = a.at(c, 4);
        for j in (c + 1)..4 {
            v -= a.at(c, j) * y.at(0, j);
        }
        y.set(0, c, v / a.at(c, c));
    }
    Ok([y.at(0, 0), y.at(0, 1), y.at(0, 2), y.at(0, 3)])
}

/// `sm_GnStation`: one station of the blend: the leader's parameter, the feet's series and
/// the faces' series there.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GnStation {
    /// The leader's parameter.
    pub s: f64,
    /// u1, v1, u2, v2 in `(sigma, eps)`.
    pub x: [Ser<f64>; 4],
    /// The feet on faces 1 and 2.
    pub f: [VSer<f64>; 2],
}

/// `sm_LeaderSeries`: the leader (centre line) at `s` as series in sigma up to `tr.s`: its
/// point, and its unit tangent (which takes one derivative more).
///
/// # Errors
///
/// Those of the curve's evaluation; `SM_ERR` where its tangent vanishes.
pub fn leader_series(c: &BSplineCurve, s: f64, tr: Trunc) -> Result<(VSer<f64>, VSer<f64>)> {
    let sigma = Trunc::new(tr.s(), 0)?;
    // Evaluate(dS, tr.s + 1, TRUE, asD, FALSE)
    let d = c.evaluate(s, tr.s() + 1, true, false)?;
    let zero = V3 {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };
    let mut l = VSer::constant(zero);
    let mut d1 = l;
    let mut fact = 1.0_f64;
    for j in 0..=tr.s() {
        if j > 0 {
            fact *= j as f64;
        }
        let (Some(dj), Some(dj1)) = (d.get(j), d.get(j + 1)) else {
            return Err(SolidError::sm_err(OP));
        };
        let cl = [dj.x, dj.y, dj.z];
        let cd = [dj1.x, dj1.y, dj1.z];
        for ((la, da), (&x, &y)) in l.a.iter_mut().zip(d1.a.iter_mut()).zip(cl.iter().zip(&cd)) {
            la.set(j, 0, x / fact);
            da.set(j, 0, y / fact);
        }
    }
    let inv = d1.dot(&d1, sigma).inv_sqrt(sigma)?;
    let t = d1.times(&inv, sigma);
    Ok((l, t))
}

/// `sm_GnSolveStation`: the station at leader parameter `s`: the feet solved by Newton
/// from the seeds (u1, v1, u2, v2), then their series in `(sigma, eps)` up to `tr`, order by
/// order with the station's Jacobian.
///
/// # Errors
///
/// `SM_ERR` when Newton does not converge in 50 steps ("the feet of a station did not
/// converge"), the system is singular, or an evaluation fails.
#[allow(clippy::too_many_arguments)]
pub fn gn_solve_station(
    k: &Kernel,
    faces: [&Surface; 2],
    leader: &BSplineCurve,
    s: f64,
    seed: [f64; 4],
    r: [f64; 2],
    side: [f64; 2],
    tr: Trunc,
    tol: f64,
) -> Result<GnStation> {
    if tr.s() > MAX_S {
        return Err(SolidError::sm_err(OP));
    }
    let lk = tr.s() + tr.e() + 1;
    let (l, t) = leader_series(leader, s, tr)?;
    let [f0, f1] = faces;
    let partials_at = |x: &[f64; 4], order: usize| -> Result<[Partials<f64>; 2]> {
        Ok([
            surface_partials(k, f0, Point2::new(x[0], x[1]), order)?,
            surface_partials(k, f1, Point2::new(x[2], x[3]), order)?,
        ])
    };

    // the feet at tau = 1, by Newton (the leader at sigma = 0 only)
    let mut x = seed;
    let point = Trunc::new(0, 0)?;
    let mut converged = false;
    let mut it = 0;
    while it < 50 && !converged {
        let pp = partials_at(&x, 2)?;
        let [p0, p1] = &pp;
        let sys = GnSystem {
            p: [p0, p1],
            leader: l,
            tangent: t,
            r,
            side,
            hold: false,
        };
        let sx = x.map(Ser::constant);
        let (g, _) = gn_evaluate(&sys, &sx, point)?;
        let jac = gn_jacobian(&sys, &sx)?;
        let mut b = [0.0_f64; 4];
        let mut res = 0.0_f64;
        for (bi, gi) in b.iter_mut().zip(&g) {
            *bi = -gi.at(0, 0);
            res = smax(res, bi.abs());
        }
        let dx = solve4(&jac, &b)?;
        let mut step = 0.0_f64;
        for (xq, &dq) in x.iter_mut().zip(&dx) {
            *xq += dq;
            step = smax(step, dq.abs() / (1.0 + xq.abs()));
        }
        // converged: the residual within the tolerance and the step at rounding
        if res <= tol && step <= 1.0e-12 {
            converged = true;
        }
        it += 1;
    }
    if !converged {
        return Err(SolidError::sm_err(OP));
    }

    // the series, order by order: each coefficient of G is J x[i][k] plus the lower ones'
    let pp = partials_at(&x, lk)?;
    let [p0, p1] = &pp;
    let sys = GnSystem {
        p: [p0, p1],
        leader: l,
        tangent: t,
        r,
        side,
        hold: false,
    };
    let mut sx = x.map(Ser::constant);
    let jac = gn_jacobian(&sys, &sx)?;
    // (the coefficients of one total degree do not enter each other's: one evaluation
    // solves them all)
    for tot in 1..=(tr.s() + tr.e()) {
        let (g, _) = gn_evaluate(&sys, &sx, tr)?;
        for i in 0..=tr.s().min(tot) {
            let kk = tot - i;
            if kk > tr.e() {
                continue;
            }
            let b = [
                g[0].at(i, kk),
                g[1].at(i, kk),
                g[2].at(i, kk),
                g[3].at(i, kk),
            ]
            .map(|v| -v);
            let y = solve4(&jac, &b)?;
            for (sq, &yq) in sx.iter_mut().zip(&y) {
                sq.set(i, kk, yq);
            }
        }
    }
    let (_, f) = gn_evaluate(&sys, &sx, tr)?;
    Ok(GnStation { s, x: sx, f })
}

/// A unit vector check helper for tests and callers: `Vec3` of a `V3<f64>`.
#[must_use]
pub fn vec3(v: V3<f64>) -> Vec3 {
    Vec3::new(v.x, v.y, v.z)
}

#[cfg(test)]
#[allow(clippy::needless_range_loop)]
mod tests {
    use super::*;
    use crate::blend::fillet_section::interval::Ivl;
    use crate::geom::extent2d::Extent2d;
    use crate::math::Vec2;
    use crate::surface::plane::Plane;

    fn plane(x_axis: Vec3, y_axis: Vec3) -> Surface {
        Surface::from(Plane::new(
            Vec3::ZERO,
            x_axis,
            y_axis,
            Vec2::new(1.0, 1.0),
            &Extent2d::from_bounds(-10.0, -10.0, 10.0, 10.0),
        ))
    }

    #[test]
    fn cubic_bezier_basis_and_derivatives() {
        let u = [0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0];
        let k = span(&u, 3, 4, 0.5);
        assert_eq!(k, 3);
        let d = ders_basis(&u, k, 3, 0.5, 2);
        let want0 = [0.125, 0.375, 0.375, 0.125];
        // B' = 3 (B2_{i-1} - B2_i), B2 at 1/2 = [1/4, 1/2, 1/4]
        let want1 = [-0.75, -0.75, 0.75, 0.75];
        // B'' = 6 (B1_{i-2} - 2 B1_{i-1} + B1_i), B1 at 1/2 = [1/2, 1/2]
        let want2 = [3.0, -3.0, -3.0, 3.0];
        for j in 0..4 {
            assert!((d[0][j] - want0[j]).abs() < 1e-15);
            assert!((d[1][j] - want1[j]).abs() < 1e-14);
            assert!((d[2][j] - want2[j]).abs() < 1e-14);
        }
        // the end span continues past the knots
        assert_eq!(span(&u, 3, 4, 1.5), 3);
        assert_eq!(span(&u, 3, 4, -0.5), 3);
        // an interval argument encloses the double one
        let di = ders_basis(&u, k, 3, Ivl::from(0.5), 2);
        for r in 0..=2 {
            for j in 0..4 {
                assert!(di[r][j].lo <= d[r][j] && d[r][j] <= di[r][j].hi);
            }
        }
    }

    /// A plane's partials by its net are the plane's, inside and past its domain.
    #[test]
    fn plane_partials_by_its_net() {
        let k = Kernel::default();
        let s = plane(Vec3::X, Vec3::Y);
        for (u, v) in [(0.5, 0.25), (12.0, -11.0)] {
            let p = surface_partials(&k, &s, Point2::new(u, v), 2).unwrap();
            assert!((vec3(p.at(0, 0)) - Vec3::new(u, v, 0.0)).length() < 1e-12);
            assert!((vec3(p.at(1, 0)) - Vec3::X).length() < 1e-12);
            assert!((vec3(p.at(0, 1)) - Vec3::Y).length() < 1e-12);
            assert!(vec3(p.at(1, 1)).length() < 1e-12);
            assert!(vec3(p.at(2, 0)).length() < 1e-12);
        }
    }

    /// Two planes at a square corner, the ball of radius r on the edge along y: the point
    /// at distance tau r from both is (tau r, y, tau r), its feet (tau r, y, 0) and
    /// (0, y, tau r), linear in tau, and y = L s along the leader.
    #[test]
    fn square_corner_station_is_the_closed_form() {
        let k = Kernel::default();
        let f1 = plane(Vec3::X, Vec3::Y); // z = 0, (u, v) = (x, y), normal +z
        let f2 = plane(Vec3::Y, Vec3::Z); // x = 0, (u, v) = (y, z), normal +x
        let (r, len) = (2.0, 5.0);
        let leader =
            BSplineCurve::create_line_segment(3, Vec3::new(r, 0.0, r), Vec3::new(r, len, r))
                .unwrap();
        let s = 0.3;
        let y = len * s;
        let tr = Trunc::new(1, 3).unwrap();
        let st = gn_solve_station(
            &k,
            [&f1, &f2],
            &leader,
            s,
            [r + 0.1, y - 0.2, y + 0.1, r - 0.3],
            [r, r],
            [1.0, 1.0],
            tr,
            1e-12,
        )
        .unwrap();
        let want_x = [r, y, y, r];
        for q in 0..4 {
            assert!((st.x[q].c[0][0] - want_x[q]).abs() < 1e-12, "{q}");
        }
        // d/d eps: u1 = x = tau r, v2 = z = tau r; d/d sigma: v1 = u2 = y = L s
        assert!((st.x[0].c[0][1] - r).abs() < 1e-12);
        assert!((st.x[3].c[0][1] - r).abs() < 1e-12);
        assert!((st.x[1].c[1][0] - len).abs() < 1e-12);
        assert!((st.x[2].c[1][0] - len).abs() < 1e-12);
        // everything else vanishes: the feet are linear in tau and in s
        for q in 0..4 {
            for i in 0..=1 {
                for e in 0..=3 {
                    if i + e >= 2 || (i, e) == (0, 1) && (q == 1 || q == 2) {
                        assert!(st.x[q].c[i][e].abs() < 1e-11, "{q} {i} {e}");
                    }
                }
            }
        }
        // the feet: F1 = (tau r, y, 0), F2 = (0, y, tau r)
        let f = |face: usize, i: usize, e: usize| {
            Vec3::new(
                st.f[face].a[0].c[i][e],
                st.f[face].a[1].c[i][e],
                st.f[face].a[2].c[i][e],
            )
        };
        assert!((f(0, 0, 0) - Vec3::new(r, y, 0.0)).length() < 1e-12);
        assert!((f(1, 0, 0) - Vec3::new(0.0, y, r)).length() < 1e-12);
        assert!((f(0, 0, 1) - Vec3::new(r, 0.0, 0.0)).length() < 1e-12);
        assert!((f(1, 0, 1) - Vec3::new(0.0, 0.0, r)).length() < 1e-12);
        assert!((f(0, 1, 0) - Vec3::new(0.0, len, 0.0)).length() < 1e-12);
        assert!(f(0, 0, 2).length() < 1e-11 && f(1, 0, 3).length() < 1e-11);
    }

    #[test]
    fn singular_system_is_refused() {
        let a = [
            [1.0, 2.0, 0.0, 0.0],
            [2.0, 4.0, 0.0, 0.0],
            [0.0; 4],
            [0.0; 4],
        ];
        assert!(solve4(&a, &[1.0; 4]).is_err());
        let a = [
            [2.0, 0.0, 0.0, 1.0],
            [0.0, 3.0, 0.0, 0.0],
            [0.0, 0.0, 4.0, 0.0],
            [1.0, 0.0, 0.0, 1.0],
        ];
        let y = solve4(&a, &[3.0, 3.0, 4.0, 2.0]).unwrap();
        for (yi, w) in y.iter().zip([1.0, 1.0, 1.0, 1.0]) {
            assert!((yi - w).abs() < 1e-15);
        }
    }
}
