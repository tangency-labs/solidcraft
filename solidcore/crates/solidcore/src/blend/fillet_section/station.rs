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
//!
//! Deviations: an enclosure (an interval or a Taylor model) takes each face's offset point
//! `S + r N` as one Taylor expansion in the foot's parameters ([`offset_taylor`],
//! tangency-labs/solidcraft#16652), where the fork sums the expansions of `S` and `r N`: the
//! same algebra, but only the first keeps their cancellation near the face's focal
//! distance. It does so only where the offset is not proved short of that distance
//! (`offset_near_focal`, #19306); elsewhere, and in the floating-point jets, it keeps the
//! fork's sum.

use std::borrow::Cow;

use super::series::{GnScalar, InvSqrtSeries, Ser, Trunc, VSer, MAX_E, MAX_K, MAX_S, STORE_E, V3};
use crate::error::blend::{BlendFailure, SectionDegeneracy, SectionLimit, SectionProof};
use crate::error::{Failure, Result, SolidError};
use crate::kernel::Kernel;
use crate::math::{Point2, Vec3};
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::b_spline_surface::p1::SurfParamType;
use crate::nurbs::b_spline_surface::BSplineSurface;
use crate::nurbs::b_spline_surface_n_lib::p2::EvaluateFlags;
use crate::nurbs::knots::Side;
use crate::nurbs::math::NOW;
use crate::nurbs::nurbs_crv::MAX_DERIV;
use crate::surface::procedural_blend::NetScope;
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
        // "SmGnSectionFSG: a face's NURBS data do not agree"
        if p < 1 || q < 1 {
            return Err(SolidError::blend(
                OP,
                BlendFailure::SectionUnsupported(SectionLimit::DegreeBelowOne),
            ));
        }
        if u.len() != lnu + p + 1 || v.len() != lnv + q + 1 {
            return Err(SolidError::blend(
                OP,
                BlendFailure::Defect {
                    why: "a face's knots do not number its control points plus its order: a \
                          B-spline surface the kernel holds keeps the two consistent",
                },
            ));
        }
        if cps.len() != lnu * lnv {
            return Err(SolidError::blend(
                OP,
                BlendFailure::Defect {
                    why: "a face's control net is not its rows by its columns: a B-spline \
                          surface the kernel holds keeps its net that size",
                },
            ));
        }
        let mut pw = Vec::new();
        for c in &cps {
            let rational = c.w != NOW;
            let w = if rational { c.w } else { 1.0 };
            pw.extend_from_slice(&[c.x, c.y, c.z, w]);
            let positive = w > 0.0;
            if !positive {
                // "SmGnSectionFSG: a face's weights are not positive"
                return Err(SolidError::blend(
                    OP,
                    BlendFailure::SectionUnsupported(SectionLimit::FaceWeightNotPositive),
                ));
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

/// `sm_SidedSpan` (3bc54a4): [`span`], except at a knot line (within `1e-9 (1 + |x|)` of
/// it), where `side` -1 takes the piece before it and +1 the piece after it; 0 the span
/// [`span`] finds.
#[must_use]
pub fn sided_span(u: &[f64], p: usize, n: usize, x: f64, side: i32) -> usize {
    let mut k = span(u, p, n, x);
    let tol = 1.0e-9 * (1.0 + x.abs());
    let at = |i: usize| knot(u, i as isize);
    if side < 0 && (x - at(k)).abs() <= tol {
        while k > p && at(k - 1).partial_cmp(&at(k)) != Some(core::cmp::Ordering::Less) {
            k -= 1;
        }
        if k > p {
            k -= 1;
        }
    } else if side > 0 && k + 1 < n && (x - at(k + 1)).abs() <= tol {
        k += 1;
        while k + 1 < n && at(k).partial_cmp(&at(k + 1)) != Some(core::cmp::Ordering::Less) {
            k += 1;
        }
    }
    k
}

/// `sm_DersBasis`: the non-zero basis functions `N_{k-p..k}` and their derivatives up to
/// `d` at `x` in span `k`, `ders[d][j]` (the NURBS book's A2.3, its knot differences taken
/// from the knots directly, in `T`: for an interval `T` every result holds the exact one).
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
            // the knots are exact: their difference is rounded in T (outward for an
            // interval), never as a double first (#2559)
            ndu.set(
                j,
                r,
                T::of(knot(u, k + r + 1)) - T::of(knot(u, k + 1 - j + r)),
            );
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
        SurfaceKind::ProceduralBlend(pb) => {
            let b = pb.net();
            Some(Cow::Borrowed(b))
        }
        SurfaceKind::BallPatch(bp) => {
            let b = bp.net();
            Some(Cow::Borrowed(b))
        }
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

/// The station solve's stopping step: Newton stops once its last step in every foot
/// parameter x is at most `STATION_STEP (1 + |x|)` (with the residual within the
/// tolerance). Converging quadratically, the root is then within that step of the
/// computed feet, which bounds the stations' error ([`super::recertify::precision_floor`]).
pub const STATION_STEP: f64 = 1.0e-12;

/// `sm_SurfacePartials`: the partials of a face's surface at `uv` up to total order `lk`:
/// a NURBS (every analytic surface is one, evaluated in its net's parameters) by its net,
/// continued past its knots; any other surface, and a blend by its definition (whose net
/// is only its approximation, 014f66f), by its own evaluator, a definition as S whatever
/// [`NetScope`] the caller is in (#2510).
///
/// # Errors
///
/// `SM_ERR` for `lk > MAX_K`; those of [`Nurbs::of`] and the surface's evaluator.
pub fn surface_partials(k: &Kernel, s: &Surface, uv: Point2, lk: usize) -> Result<Partials<f64>> {
    surface_partials_sided(k, s, uv, lk, [0, 0])
}

/// [`surface_partials`] with its `lSideU`, `lSideV` (3bc54a4): at a NURBS face's knot
/// line, -1 the piece before it, +1 the piece after it ([`sided_span`]).
///
/// # Errors
///
/// As [`surface_partials`].
pub fn surface_partials_sided(
    k: &Kernel,
    s: &Surface,
    uv: Point2,
    lk: usize,
    sides: [i32; 2],
) -> Result<Partials<f64>> {
    surface_partials_pinned(k, s, uv, uv, lk, sides)
}

/// [`surface_partials_sided`] with the pieces a side names chosen at `pin`, not at `uv`
/// (not the fork's): a sided station solve keeps, for every iterate, the pieces its sides
/// name at its seed, which its callers place on the knot line. Chosen at each iterate,
/// an iterate as far from the line as the margin of [`sided_span`] took the other piece,
/// and Newton cycled between the two (feet 2e-9 off the old boundary of a face's
/// extension, which is only G1 there: the station failed). A coordinate without a side
/// takes the span of `uv`, as [`surface_partials_sided`].
///
/// # Errors
///
/// As [`surface_partials`].
pub fn surface_partials_pinned(
    k: &Kernel,
    s: &Surface,
    uv: Point2,
    pin: Point2,
    lk: usize,
    sides: [i32; 2],
) -> Result<Partials<f64>> {
    if lk > MAX_K {
        return Err(SolidError::blend(
            OP,
            BlendFailure::SectionUnsupported(SectionLimit::OrderPastSeries {
                order: lk,
                most: MAX_K,
            }),
        ));
    }
    let definition = matches!(s.kind(), SurfaceKind::ProceduralBlend(_));
    let bs = if definition { None } else { as_b_spline(k, s)? };
    if let Some(bs) = bs {
        let n = Nurbs::of(&bs)?;
        let at = |x: f64, p: f64, side: i32| if side == 0 { x } else { p };
        let ku = sided_span(&n.u, n.p, n.nu, at(uv.x, pin.x, sides[0]), sides[0]);
        let kv = sided_span(&n.v, n.q, n.nv, at(uv.y, pin.y, sides[1]), sides[1]);
        return Ok(nurbs_partials(&n, ku, kv, uv.x, uv.y, lk));
    }
    // Evaluate(crUV, lK, lK, lSideU >= 0, lSideV >= 0, TRUE, sD, FALSE): a side, the upper
    // interval of a parameter (FromLeft TRUE) or the lower one (c5b6bb7)
    let side_of = |q: i32| if q < 0 { Side::Right } else { Side::Left };
    let flags = EvaluateFlags {
        u_side: side_of(sides[0]),
        v_side: side_of(sides[1]),
        only_upper_half: true,
        non_zero_tangents: false,
    };
    // a station is its definition's, exact to rounding inside any caller's scope: a face
    // that is a definition is solved on S, never its net (#2510)
    let d = {
        let _exact = NetScope::exact(k);
        s.evaluate(k, uv, lk, lk, flags)?
    };
    let mut out = Partials::default();
    for a in 0..=lk {
        for b in 0..=(lk - a) {
            let v = d.get(a).and_then(|r| r.get(b)).copied().ok_or_else(|| {
                SolidError::blend(
                    OP,
                    BlendFailure::Defect {
                        why: "the surface's evaluation returned fewer partials than asked: it \
                              returns every partial up to the orders asked",
                    },
                )
            })?;
            out.set(a, b, V3::from_vec3(v));
        }
    }
    Ok(out)
}

/// `sm_GnSystem`: the blend's system at a station: the two faces' partials at the feet, the
/// leader's point and unit tangent as series in sigma, the offsets `r_i` and the sides
/// `s_i`. `hold`: the offsets held at `tau = 1` (no eps in them).
#[derive(Clone, Debug)]
pub struct GnSystem<'a, T: GnScalar> {
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
    /// `pR`: a ball whose radius varies, `r(s)` as a series in sigma (`r` unused), or
    /// none (014f66f).
    pub law: Option<&'a Ser<T>>,
    /// Equal setbacks measured at the current oriented normals, instead of fixed offsets
    /// or a radius prescribed along the leader.
    pub equal_setback: Option<f64>,
}

/// The Taylor coefficients `W[a][b]` (`a + b <= deg`) of a face's offset point
/// `W(u, v) = S(u, v) + d0 N(u, v)` about the point its partials `pf` were taken at (or
/// over the box they enclose): `W(u0 + du, v0 + dv) = sum W[a][b] du^a dv^b`, `N` the unit
/// normal `S_u x S_v / |S_u x S_v|`. Not the fork's (#16652): the offset taken as one
/// expansion, so an enclosure keeps the cancellation between `S` and `d0 N`, where the
/// sum of their two expansions loses it (each enclosed apart, a coefficient's width is
/// `|S_ab| + |d0 N_ab|` where the true value is their difference: near the face's focal
/// distance, a cylinder of radius R offset by `d0 = -r` near R, every order's width came
/// out `(R + r) / (R - r)` times too wide).
///
/// The algebra, all in `T` (each operation rounded outward for an enclosure):
/// `t[a][b] = P[a][b] / (a! b!)` (the face's own Taylor coefficients, `1 / (a! b!)`
/// rounded in T as [`gn_evaluate`] takes it), `S_u`'s and `S_v`'s coefficients
/// `(a + 1) t[a + 1][b]` and `(b + 1) t[a][b + 1]`, their cross product `n` and `h = n . n`
/// by Cauchy products, and `g = h^(-1/2)` by the recurrence of its homogeneous parts from
/// `h Dg = -g Dh / 2` (`D = du d/du + dv d/dv`, the degree operator):
/// `g_m = -(1 / (m h_0)) sum_{j < m} ((m + j) / 2) h_(m-j) g_j`, `g_0 = 1 / sqrt(h_0)`;
/// then `N = n g` and `W = t + d0 N`. Each coefficient depends only on lower ones, so
/// `W[a][b]` is the same whatever `deg` it is computed to. The partials are read to total
/// order `deg + 1`.
///
/// # Errors
///
/// `SM_ERR` where the normal vanishes (`h_0` not positive), as [`Ser::inv_sqrt`].
pub fn offset_taylor<T: GnScalar>(pf: &Partials<T>, d0: T, deg: usize) -> Result<Vec<V3<T>>> {
    let zero = T::of(0.0);
    let vz = V3 {
        x: zero,
        y: zero,
        z: zero,
    };
    let k1 = deg + 1;
    let w = k1 + 1;
    let idx = |a: usize, b: usize| a * w + b;
    let get = |v: &[V3<T>], a: usize, b: usize| v.get(idx(a, b)).copied().unwrap_or(vz);
    let gets = |v: &[T], a: usize, b: usize| v.get(idx(a, b)).copied().unwrap_or(zero);
    // the face's Taylor coefficients to total degree deg + 1
    let mut t = vec![vz; w * w];
    let mut fa = 1.0_f64;
    for a in 0..=k1 {
        if a > 0 {
            fa *= a as f64;
        }
        let mut fb = 1.0_f64;
        for b in 0..=(k1 - a) {
            if b > 0 {
                fb *= b as f64;
            }
            let sf = T::of(1.0) / T::of(fa * fb);
            let p = pf.at(a, b);
            if let Some(x) = t.get_mut(idx(a, b)) {
                *x = V3 {
                    x: p.x * sf,
                    y: p.y * sf,
                    z: p.z * sf,
                };
            }
        }
    }
    // S_u's and S_v's, to total degree deg
    let mut su = vec![vz; w * w];
    let mut sv = vec![vz; w * w];
    for a in 0..=deg {
        for b in 0..=(deg - a) {
            let (cu, cv) = (T::of((a + 1) as f64), T::of((b + 1) as f64));
            let (tu, tv) = (get(&t, a + 1, b), get(&t, a, b + 1));
            if let (Some(x), Some(y)) = (su.get_mut(idx(a, b)), sv.get_mut(idx(a, b))) {
                *x = V3 {
                    x: tu.x * cu,
                    y: tu.y * cu,
                    z: tu.z * cu,
                };
                *y = V3 {
                    x: tv.x * cv,
                    y: tv.y * cv,
                    z: tv.z * cv,
                };
            }
        }
    }
    // n = S_u x S_v and h = n . n
    let mut n = vec![vz; w * w];
    let mut h = vec![zero; w * w];
    for a in 0..=deg {
        for b in 0..=(deg - a) {
            let mut c = vz;
            for i in 0..=a {
                for j in 0..=b {
                    let (p, q) = (get(&su, i, j), get(&sv, a - i, b - j));
                    c.x = c.x + (p.y * q.z - p.z * q.y);
                    c.y = c.y + (p.z * q.x - p.x * q.z);
                    c.z = c.z + (p.x * q.y - p.y * q.x);
                }
            }
            if let Some(x) = n.get_mut(idx(a, b)) {
                *x = c;
            }
        }
    }
    for a in 0..=deg {
        for b in 0..=(deg - a) {
            let mut c = zero;
            for i in 0..=a {
                for j in 0..=b {
                    let (p, q) = (get(&n, i, j), get(&n, a - i, b - j));
                    c = c + (p.x * q.x + p.y * q.y + p.z * q.z);
                }
            }
            if let Some(x) = h.get_mut(idx(a, b)) {
                *x = c;
            }
        }
    }
    // g = h^(-1/2), degree by degree
    let h0 = gets(&h, 0, 0);
    // 1 / sqrt(h0), refused where the normal vanishes, as the series' own inverse root
    let g0 = Ser::constant(h0).inv_sqrt(Trunc::new(0, 0)?)?.at(0, 0);
    let mut g = vec![zero; w * w];
    if let Some(x) = g.get_mut(idx(0, 0)) {
        *x = g0;
    }
    for m in 1..=deg {
        let scale = T::of(-1.0) / (T::of(m as f64) * h0);
        for a in 0..=m {
            let b = m - a;
            let mut c = zero;
            // g_j's coefficients (p, q), p + q = j < m, times h's at (a - p, b - q)
            for p in 0..=a {
                for q in 0..=b {
                    let j = p + q;
                    if j >= m {
                        continue;
                    }
                    let f = T::of(0.5 * (m + j) as f64);
                    c = c + f * gets(&h, a - p, b - q) * gets(&g, p, q);
                }
            }
            if let Some(x) = g.get_mut(idx(a, b)) {
                *x = scale * c;
            }
        }
    }
    // W = t + d0 n g
    let mut out = vec![vz; (deg + 1) * (deg + 1)];
    for a in 0..=deg {
        for b in 0..=(deg - a) {
            let mut c = vz;
            for i in 0..=a {
                for j in 0..=b {
                    let (p, q) = (get(&n, i, j), gets(&g, a - i, b - j));
                    c.x = c.x + p.x * q;
                    c.y = c.y + p.y * q;
                    c.z = c.z + p.z * q;
                }
            }
            let ta = get(&t, a, b);
            if let Some(x) = out.get_mut(a * (deg + 1) + b) {
                *x = V3 {
                    x: ta.x + d0 * c.x,
                    y: ta.y + d0 * c.y,
                    z: ta.z + d0 * c.z,
                };
            }
        }
    }
    Ok(out)
}

/// How far towards a face's focal distance [`offset_near_focal`] lets an offset reach and
/// still take it as the sum of the expansions of `S` and `d0 N`: a quarter of the way,
/// `d0 k <= 1/4` for both principal curvatures `k`. Measured on a cylinder's rim
/// chamfered by c (radius 10, `d0 k = c / 10`), the joint expansion certifies with the same
/// work at 0.1 and 0.3 (5275 and 5183 cancel-token polls against the sum's 5275 and 5219),
/// in 1.5 times the time, and with less from 0.5 on (5490 against 6338, half the time);
/// a round of 1 on a round of 3 (`d0 k = 1/3`) takes 34 spans with it, 37 without.
const FOCAL_REACH: f64 = 0.25;

/// Whether an enclosure takes a face's offset point `S + d0 N` as one expansion
/// ([`offset_taylor`], #16652) rather than as the sum of the expansions of `S` and `d0 N`
/// (#19306). The two are the same algebra, each an enclosure; the joint one costs more (its
/// own coefficients at every evaluation, and one more series composed with the feet) and
/// is only tighter where the sum loses a cancellation. The offset's first partials are
/// `W_u = (I - d0 W) S_u`, `W` the shape operator, whose eigenvalues `1 - d0 k_i` (`k_i`
/// the principal curvatures along N) vanish at a focal distance; the sum's widths grow as
/// `|S_u| + |d0 N_u|`, at most `(1 + d0 k) / (1 - d0 k)` times the joint ones' where
/// `0 <= d0 k < 1`, and no wider where `d0 k <= 0` (the offset away from the centre of
/// curvature). So the sum is kept where `d0 k_i <= FOCAL_REACH` (a ratio of at most 5/3)
/// is proved for both curvatures at every point the partials `pf` enclose, as
/// `I - (d0 / FOCAL_REACH) W` positive definite:
/// `A = t X [[E, F], [F, G]] - d0 [[l, m], [m, n]]` (`t = FOCAL_REACH`,
/// `X = |S_u x S_v|`, `l = S_uu . (S_u x S_v)`, ...) with `A_11` and `det A` proved
/// positive in `T` (`det A = t^2 X^4 - t d0 X H + d0^2 K`, the form whose sign is the
/// offset's regularity in `offset::construction::sheets`, there with `t = 1`). Not proved
/// (near a focal distance, a box too wide to tell, a NaN), the joint expansion is taken.
/// The test reads the partials to order 2: a caller that has fewer (0 past what it
/// computed) evaluates at degree 0, where the two differ only in their rounding. Always
/// false where `T` takes the sum ([`GnScalar::JOINT_OFFSET`]).
fn offset_near_focal<T: GnScalar>(pf: &Partials<T>, d0: T) -> bool {
    if !T::JOINT_OFFSET {
        return false;
    }
    let dot = |a: V3<T>, b: V3<T>| a.x * b.x + a.y * b.y + a.z * b.z;
    let (su, sv) = (pf.at(1, 0), pf.at(0, 1));
    let n = V3 {
        x: su.y * sv.z - su.z * sv.y,
        y: su.z * sv.x - su.x * sv.z,
        z: su.x * sv.y - su.y * sv.x,
    };
    let tx = T::of(FOCAL_REACH) * dot(n, n).sqrt();
    let a11 = tx * dot(su, su) - d0 * dot(pf.at(2, 0), n);
    let a12 = tx * dot(su, sv) - d0 * dot(pf.at(1, 1), n);
    let a22 = tx * dot(sv, sv) - d0 * dot(pf.at(0, 2), n);
    !(a11.is_positive() && (a11 * a22 - a12 * a12).is_positive())
}

/// The offset's constant term `d0` (its series' at `sigma^0 eps^0`): `r_i s_i`, or the
/// law's radius there times the side.
fn offset_d0<T: GnScalar>(sys: &GnSystem<'_, T>, rf: f64, sidef: f64) -> T {
    match sys.law {
        Some(law) => law.at(0, 0) * T::of(sidef),
        None => T::of(rf * sidef),
    }
}

/// Whether the offset's series has terms past its constant: a law's radius varies with
/// sigma, and with the offsets not held the offset is `tau r_i s_i`, `tau = 1 + eps`.
fn offset_varies<T: GnScalar>(sys: &GnSystem<'_, T>, tr: Trunc) -> bool {
    sys.law.is_some() || (!sys.hold && tr.e() >= 1)
}

/// The offset point `V = W + (d - d0) N` from the joint expansion's series `wv` (`W`
/// composed with the feet, [`offset_taylor`]), `N = nrm inv_len`, `d` the offset's series:
/// `S + d N` with `S + d0 N` taken as one. `varies`: [`offset_varies`] (otherwise
/// `d - d0` is 0 and `V = W`).
fn offset_rest<T: GnScalar>(
    wv: VSer<T>,
    nrm: &VSer<T>,
    inv_len: &Ser<T>,
    d: &Ser<T>,
    varies: bool,
    tr: Trunc,
) -> VSer<T> {
    if !varies {
        return wv;
    }
    wv.add(&nrm.times(&inv_len.mul(&offset_part(d), tr), tr), tr)
}

/// `d - d0`: the offset's series less its constant term (taken in [`offset_taylor`]).
fn offset_part<T: GnScalar>(d: &Ser<T>) -> Ser<T> {
    let mut dd = d.clone();
    dd.set(0, 0, T::of(0.0));
    dd
}

/// `W[a][b]` of [`offset_taylor`]'s coefficients computed to degree `deg` (0 past it).
fn offset_at<T: GnScalar>(w: &[V3<T>], deg: usize, a: usize, b: usize) -> V3<T> {
    let zero = T::of(0.0);
    let vz = V3 {
        x: zero,
        y: zero,
        z: zero,
    };
    if a + b > deg {
        return vz;
    }
    w.get(a * (deg + 1) + b).copied().unwrap_or(vz)
}

/// An equal setback d has radius d * (1 + q) / sqrt((1 - q) * (1 + q)), where
/// q is the dot product of the unit normals towards the centre. Both factors must be
/// positive over the station or enclosure. Treating q as a series retains its dependence
/// on the two feet in the Jacobian as well as in the higher coefficients.
fn equal_setback_radius<T: GnScalar>(
    normals: &[VSer<T>; 2],
    side: [f64; 2],
    distance: f64,
    tr: Trunc,
) -> Result<Ser<T>> {
    if !(distance.is_finite() && distance > 0.0) {
        return Err(SolidError::invalid_input(
            OP,
            "the chamfer's setback is finite and positive",
        ));
    }
    let [first, second] = normals;
    let [side_first, side_second] = side;
    let q = first
        .dot(second, tr)
        .scale(T::of(side_first * side_second), tr);
    let one = Ser::constant(T::of(1.0));
    let one_plus = one.add(&q, tr);
    let one_minus = one.sub(&q, tr);
    if !(one_plus.at(0, 0).is_positive() && one_minus.at(0, 0).is_positive()) {
        return Err(SolidError::blend(
            OP,
            BlendFailure::SectionNotProved(SectionProof::RadiusPositive),
        ));
    }
    let inv_sqrt = one_plus.mul(&one_minus, tr).inv_sqrt(tr)?;
    Ok(one_plus.mul(&inv_sqrt, tr).scale(T::of(distance), tr))
}

/// G's four series and the two feet's (`sm_GnEvaluate`'s `rG`, `rF`).
pub type GnValues<T> = ([Ser<T>; 4], [VSer<T>; 2]);

/// [`GnValues`] and V (`sm_GnEvaluate`'s `pV`).
pub type GnValuesV<T> = ([Ser<T>; 4], [VSer<T>; 2], VSer<T>);

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
    let (g, f, _) = gn_evaluate_v(sys, x, tr)?;
    Ok((g, f))
}

/// `sm_GnEvaluate` with its `pV`: G, the feet `F_i`, and the point `V(tau)` off face 1
/// (the ball's centre at `tau = 1`), as series.
///
/// # Errors
///
/// `SM_ERR` where a face's normal vanishes at a foot.
pub fn gn_evaluate_v<T: GnScalar>(
    sys: &GnSystem<'_, T>,
    x: &[Ser<T>; 4],
    tr: Trunc,
) -> Result<GnValuesV<T>> {
    let (g, f, v, _) = gn_evaluate_vn(sys, x, tr)?;
    Ok((g, f, v))
}

/// `sm_GnEvaluate` with its `pV` and `pN` (3bc54a4): also the faces' unit normals at the
/// feet, as series.
///
/// # Errors
///
/// `SM_ERR` where a face's normal vanishes at a foot.
#[allow(clippy::type_complexity)]
pub fn gn_evaluate_vn<T: GnScalar>(
    sys: &GnSystem<'_, T>,
    x: &[Ser<T>; 4],
    tr: Trunc,
) -> Result<([Ser<T>; 4], [VSer<T>; 2], VSer<T>, [VSer<T>; 2])> {
    let zero = V3 {
        x: T::of(0.0),
        y: T::of(0.0),
        z: T::of(0.0),
    };
    let lm = tr.s() + tr.e();
    let mut v_out = [VSer::constant(zero), VSer::constant(zero)];
    let mut f_out = [VSer::constant(zero), VSer::constant(zero)];
    let mut n_out = [VSer::constant(zero), VSer::constant(zero)];
    let rows = x.chunks_exact(2).zip(sys.p).zip(sys.r.iter().zip(sys.side));
    for (f, ((xf, pf), (&rf, sidef))) in rows.enumerate() {
        let (Some(xu), Some(xv)) = (xf.first(), xf.get(1)) else {
            return Err(SolidError::blend(
                OP,
                BlendFailure::Defect {
                    why: "the station's system has no foot for a face: its feet are taken in \
                          exact pairs",
                },
            ));
        };
        let mut du = xu.clone();
        let mut dv = xv.clone();
        du.set(0, 0, T::of(0.0));
        dv.set(0, 0, T::of(0.0));
        // du^a and dv^b are 0 below degree a and b (du, dv have no constant term): the
        // powers and monomials past the degree computed are 0 there and left out
        let top = lm.min(tr.degree());
        // the offset point's own expansion (enclosures near a focal distance, #16652,
        // #19306): its constant offset d0
        let d0 = offset_d0(sys, rf, sidef);
        let wt = if sys.equal_setback.is_none() && offset_near_focal(pf, d0) {
            Some(offset_taylor(pf, d0, top)?)
        } else {
            None
        };
        let mut wv = VSer::constant(zero);
        let mut pu = vec![Ser::constant(T::of(1.0))];
        let mut pv = vec![Ser::constant(T::of(1.0))];
        for a in 1..=top {
            let (Some(lu), Some(lv)) = (pu.last(), pv.last()) else {
                return Err(SolidError::blend(
                    OP,
                    BlendFailure::Defect {
                        why: "the face's powers lack the previous order: they start with the \
                              0th and each order pushes its own before the next is asked",
                    },
                ));
            };
            pu.push(lu.mul_valued(&du, a - 1, 1, tr));
            pv.push(lv.mul_valued(&dv, a - 1, 1, tr));
        }
        let mut s = VSer::constant(zero);
        let mut su = s.clone();
        let mut sv = s.clone();
        let mut fa = 1.0_f64;
        for (a, pua) in pu.iter().enumerate() {
            if a > 0 {
                fa *= a as f64;
            }
            let mut fb = 1.0_f64;
            for (b, pvb) in pv.iter().enumerate().take(top - a + 1) {
                if b > 0 {
                    fb *= b as f64;
                }
                let mono = pua.mul_valued(pvb, a, b, tr);
                // a! b! is exact, 1 / (a! b!) is rounded in T (#2559)
                let sf = T::of(1.0) / T::of(fa * fb);
                let cs = [pf.at(a, b), pf.at(a + 1, b), pf.at(a, b + 1)];
                let outs: [&mut VSer<T>; 3] = [&mut s, &mut su, &mut sv];
                for (c, out) in cs.iter().zip(outs) {
                    let cv = [c.x, c.y, c.z];
                    for (o, &cj) in out.a.iter_mut().zip(&cv) {
                        // o + mono (cj sf), each coefficient rounded as `add` of `scale`
                        o.add_scaled_from(&mono, cj * sf, a + b, tr);
                    }
                }
                if let Some(wt) = &wt {
                    let c = offset_at(wt, top, a, b);
                    for (o, &cj) in wv.a.iter_mut().zip(&[c.x, c.y, c.z]) {
                        o.add_scaled_from(&mono, cj, a + b, tr);
                    }
                }
            }
        }
        let nrm = su.cross(&sv, tr);
        let inv_len = nrm.dot(&nrm, tr).inv_sqrt(tr)?;
        if sys.equal_setback.is_some() {
            if let (Some(foot), Some(normal)) = (f_out.get_mut(f), n_out.get_mut(f)) {
                *foot = s;
                *normal = nrm.times(&inv_len, tr);
            }
            continue;
        }
        // the offset towards the ball: tau r_i s_i, tau = 1 + eps (r_i a series in sigma
        // for a law)
        let mut d = Ser::constant(T::of(rf * sidef));
        if let Some(law) = sys.law {
            for i in 0..=tr.s() {
                d.set(i, 0, law.at(i, 0) * T::of(sidef));
            }
        }
        if !sys.hold && tr.e() >= 1 {
            for i in 0..=tr.s() {
                d.set(i, 1, d.at(i, 0));
            }
        }
        let vf = if wt.is_some() {
            offset_rest(wv, &nrm, &inv_len, &d, offset_varies(sys, tr), tr)
        } else {
            s.add(&nrm.times(&inv_len.mul(&d, tr), tr), tr)
        };
        if let (Some(vo), Some(fo), Some(no)) =
            (v_out.get_mut(f), f_out.get_mut(f), n_out.get_mut(f))
        {
            *vo = vf;
            *no = nrm.times(&inv_len, tr);
            *fo = s;
        }
    }
    if let Some(distance) = sys.equal_setback {
        let radius = equal_setback_radius(&n_out, sys.side, distance, tr)?;
        let mut tau = Ser::constant(T::of(1.0));
        if !sys.hold && tr.e() >= 1 {
            tau.set(0, 1, T::of(1.0));
        }
        let offset = radius.mul(&tau, tr);
        for (((centre, foot), normal), side) in
            v_out.iter_mut().zip(&f_out).zip(&n_out).zip(sys.side)
        {
            *centre = foot.add(&normal.times(&offset.scale(T::of(side), tr), tr), tr);
        }
    }
    let [v0, v1] = &v_out;
    let diff = v0.sub(v1, tr);
    let [g0, g1, g2] = diff.a;
    let g3 = v0.sub(&sys.leader, tr).dot(&sys.tangent, tr);
    Ok(([g0, g1, g2, g3], f_out, v0.clone(), n_out))
}

/// [`gn_evaluate_vn`]'s values: G, the feet, V and the faces' unit normals.
pub type GnValuesVN<T> = ([Ser<T>; 4], [VSer<T>; 2], VSer<T>, [VSer<T>; 2]);

/// One face's series that [`GnIncremental`] keeps between evaluations.
#[derive(Clone, Debug)]
struct GnFaceSeries<T: GnScalar> {
    /// `du^a`, `dv^a`: index 0 is 1.
    pu: Vec<Ser<T>>,
    pv: Vec<Ser<T>>,
    s: VSer<T>,
    su: VSer<T>,
    sv: VSer<T>,
    nrm: VSer<T>,
    nn: Ser<T>,
    inv: InvSqrtSeries<T>,
    w: Ser<T>,
    v: VSer<T>,
    n: VSer<T>,
    /// The offset point's joint expansion composed with the feet ([`offset_taylor`]).
    wv: VSer<T>,
    /// Its coefficients, to the orders' total degree (none before the first evaluation,
    /// and none where the offset is the sum, [`offset_near_focal`]).
    wt: Option<Vec<V3<T>>>,
    /// Whether the offset is the joint expansion, decided at the first evaluation.
    joint: Option<bool>,
}

impl<T: GnScalar> GnFaceSeries<T> {
    fn new() -> Self {
        let z = Ser::constant(T::of(0.0));
        let vz = VSer {
            a: core::array::from_fn(|_| z.clone()),
        };
        let one = vec![Ser::constant(T::of(1.0))];
        Self {
            pu: one.clone(),
            pv: one,
            s: vz.clone(),
            su: vz.clone(),
            sv: vz.clone(),
            nrm: vz.clone(),
            nn: z.clone(),
            inv: InvSqrtSeries::default(),
            w: z,
            v: vz.clone(),
            n: vz.clone(),
            wv: vz,
            wt: None,
            joint: None,
        }
    }
}

/// `v.a[j]` merged from `src.a[j]` where `tr` computes.
fn merge_v<T: GnScalar>(v: &mut VSer<T>, src: &VSer<T>, tr: Trunc) {
    for (a, b) in v.a.iter_mut().zip(&src.a) {
        a.merge_computed(b, tr);
    }
}

/// [`gn_evaluate_vn`] evaluated incrementally, for a caller that evaluates the same system
/// again and again as it fills in the feet's series (the certificate's Taylor models
/// solve them order by order, [`super::feet_tm::gn_enclose_tm`]): every series
/// [`gn_evaluate_vn`] takes is kept, and an evaluation computes only the coefficients its
/// truncation computes that no earlier one has computed from the same coefficients of x.
/// Each coefficient depends only on x's coefficients at the same or lower powers of both
/// sigma and eps (see [`Trunc`]), so a kept one stays valid until x changes in that
/// quadrant, and the evaluation's operations, their order and so every value are
/// [`gn_evaluate_vn`]'s, bit for bit: the two must change together (a test compares them).
#[derive(Clone, Debug)]
pub struct GnIncremental<'s, 'a, T: GnScalar> {
    sys: &'s GnSystem<'a, T>,
    /// The orders of the kept series (`Trunc::s`, `Trunc::e`), none before the first.
    orders: Option<(usize, usize)>,
    /// The x the kept series were computed from.
    x: [Ser<T>; 4],
    /// The kept coefficients still valid ([`Trunc::bit`]s, the same for every series).
    valid: u64,
    faces: [GnFaceSeries<T>; 2],
    g: [Ser<T>; 4],
    /// `V - leader` (its dot product with the tangent reads all its lower coefficients).
    t: VSer<T>,
    /// The coefficients computed so far (each evaluation adds those of its series' it
    /// computes, counted once per position).
    computed: u64,
}

impl<'s, 'a, T: GnScalar + PartialEq> GnIncremental<'s, 'a, T> {
    /// An evaluator of `sys` with nothing kept.
    #[must_use]
    pub fn new(sys: &'s GnSystem<'a, T>) -> Self {
        let z = Ser::constant(T::of(0.0));
        Self {
            sys,
            orders: None,
            x: core::array::from_fn(|_| z.clone()),
            valid: 0,
            faces: [GnFaceSeries::new(), GnFaceSeries::new()],
            g: core::array::from_fn(|_| z.clone()),
            t: VSer {
                a: core::array::from_fn(|_| z.clone()),
            },
            computed: 0,
        }
    }

    /// The positions `(i, k)` computed so far, summed over the evaluations: the work
    /// counter of the certificate's feet Taylor models. A [`gn_evaluate_vn`] call computes
    /// every position its truncation computes.
    #[must_use]
    pub const fn computed(&self) -> u64 {
        self.computed
    }

    /// [`gn_evaluate_vn`]`(sys, x, tr)`, bit for bit.
    ///
    /// # Errors
    ///
    /// As [`gn_evaluate_vn`].
    pub fn evaluate(&mut self, x: &[Ser<T>; 4], tr: Trunc) -> Result<GnValuesVN<T>> {
        // With equal setbacks the two faces' offsets depend on both normals. The
        // per-face cache below assumes each offset is independent of the other foot.
        if self.sys.equal_setback.is_some() {
            self.computed = self
                .computed
                .saturating_add(u64::from(tr.computed_mask().count_ones()));
            return gn_evaluate_vn(self.sys, x, tr);
        }
        let out = self.evaluate_kept(x, tr);
        if out.is_err() {
            // the kept series are part way through an update
            self.valid = 0;
            self.orders = None;
        }
        out
    }

    fn evaluate_kept(&mut self, x: &[Ser<T>; 4], tr: Trunc) -> Result<GnValuesVN<T>> {
        if self.orders != Some((tr.s(), tr.e())) {
            let computed = self.computed;
            *self = Self::new(self.sys);
            self.computed = computed;
            self.orders = Some((tr.s(), tr.e()));
        }
        // a coefficient of x that changed invalidates every kept one at or above both its
        // powers
        for (xn, xo) in x.iter().zip(&self.x) {
            for i in 0..=MAX_S {
                for k in 0..=STORE_E {
                    if xn.at(i, k) != xo.at(i, k) {
                        for i2 in i..=MAX_S {
                            for k2 in k..=STORE_E {
                                self.valid &= !Trunc::bit(i2, k2);
                            }
                        }
                    }
                }
            }
        }
        self.x.clone_from(x);
        let all = tr.computed_mask();
        let btr = tr.skipping(self.valid);
        self.computed = self
            .computed
            .saturating_add(u64::from(btr.computed_mask().count_ones()));
        let lm = tr.s() + tr.e();
        let top = lm.min(tr.degree());
        let zero = Ser::constant(T::of(0.0));
        let vzero = VSer {
            a: core::array::from_fn(|_| zero.clone()),
        };
        let sys = self.sys;
        let rows = x.chunks_exact(2).zip(sys.p).zip(sys.r.iter().zip(sys.side));
        for (fs, ((xf, pf), (&rf, sidef))) in self.faces.iter_mut().zip(rows) {
            let (Some(xu), Some(xv)) = (xf.first(), xf.get(1)) else {
                return Err(SolidError::blend(
                    OP,
                    BlendFailure::Defect {
                        why: "the station's system has no foot for a face: its feet are taken \
                              in exact pairs",
                    },
                ));
            };
            let mut du = xu.clone();
            let mut dv = xv.clone();
            du.set(0, 0, T::of(0.0));
            dv.set(0, 0, T::of(0.0));
            for a in 1..=top {
                let (Some(lu), Some(lv)) = (fs.pu.get(a - 1), fs.pv.get(a - 1)) else {
                    return Err(SolidError::blend(
                        OP,
                        BlendFailure::Defect {
                            why: "the face's kept powers lack an order: they start with the \
                                  0th and each order pushes its own before the next is asked",
                        },
                    ));
                };
                let (nu, nv) = (
                    lu.mul_valued(&du, a - 1, 1, btr),
                    lv.mul_valued(&dv, a - 1, 1, btr),
                );
                if fs.pu.len() <= a {
                    fs.pu.push(zero.clone());
                    fs.pv.push(zero.clone());
                }
                let (Some(pua), Some(pva)) = (fs.pu.get_mut(a), fs.pv.get_mut(a)) else {
                    return Err(SolidError::blend(
                        OP,
                        BlendFailure::Defect {
                            why: "the face's kept powers lack an order: they start with the \
                                  0th and each order pushes its own before the next is asked",
                        },
                    ));
                };
                pua.merge_computed(&nu, btr);
                pva.merge_computed(&nv, btr);
            }
            merge_v(&mut fs.s, &vzero, btr);
            merge_v(&mut fs.su, &vzero, btr);
            merge_v(&mut fs.sv, &vzero, btr);
            // the offset point's joint expansion near a focal distance (#16652, #19306),
            // decided and its coefficients computed once per orders (each the same whatever
            // degree it is computed to, so [`gn_evaluate_vn`]'s)
            if fs.joint.is_none() {
                let d0 = offset_d0(sys, rf, sidef);
                let near = offset_near_focal(pf, d0);
                if near {
                    fs.wt = Some(offset_taylor(pf, d0, lm)?);
                }
                fs.joint = Some(near);
            }
            merge_v(&mut fs.wv, &vzero, btr);
            let mut fa = 1.0_f64;
            for (a, pua) in fs.pu.iter().enumerate().take(top + 1) {
                if a > 0 {
                    fa *= a as f64;
                }
                let mut fb = 1.0_f64;
                for (b, pvb) in fs.pv.iter().enumerate().take(top - a + 1) {
                    if b > 0 {
                        fb *= b as f64;
                    }
                    let mono = pua.mul_valued(pvb, a, b, btr);
                    let sf = T::of(1.0) / T::of(fa * fb);
                    let cs = [pf.at(a, b), pf.at(a + 1, b), pf.at(a, b + 1)];
                    let outs: [&mut VSer<T>; 3] = [&mut fs.s, &mut fs.su, &mut fs.sv];
                    for (c, out) in cs.iter().zip(outs) {
                        let cv = [c.x, c.y, c.z];
                        for (o, &cj) in out.a.iter_mut().zip(&cv) {
                            o.add_scaled_from(&mono, cj * sf, a + b, btr);
                        }
                    }
                    if let Some(wt) = &fs.wt {
                        let c = offset_at(wt, lm, a, b);
                        for (o, &cj) in fs.wv.a.iter_mut().zip(&[c.x, c.y, c.z]) {
                            o.add_scaled_from(&mono, cj, a + b, btr);
                        }
                    }
                }
            }
            merge_v(&mut fs.nrm, &fs.su.cross(&fs.sv, btr), btr);
            fs.nn.merge_computed(&fs.nrm.dot(&fs.nrm, btr), btr);
            let inv_len = fs.inv.update(&fs.nn, btr)?.clone();
            let mut d = Ser::constant(T::of(rf * sidef));
            if let Some(law) = sys.law {
                for i in 0..=tr.s() {
                    d.set(i, 0, law.at(i, 0) * T::of(sidef));
                }
            }
            if !sys.hold && tr.e() >= 1 {
                for i in 0..=tr.s() {
                    d.set(i, 1, d.at(i, 0));
                }
            }
            // (kept, as every series a later product reads: a product's coefficient reads
            // its operands' lower ones, which an incremental call does not compute again)
            let joint = fs.wt.is_some();
            let dw = if joint { offset_part(&d) } else { d };
            fs.w.merge_computed(&inv_len.mul(&dw, btr), btr);
            if joint {
                let vf = if offset_varies(sys, tr) {
                    fs.wv.add(&fs.nrm.times(&fs.w, btr), btr)
                } else {
                    fs.wv.clone()
                };
                merge_v(&mut fs.v, &vf, btr);
            } else {
                merge_v(&mut fs.v, &fs.s.add(&fs.nrm.times(&fs.w, btr), btr), btr);
            }
            merge_v(&mut fs.n, &fs.nrm.times(&inv_len, btr), btr);
        }
        let [f0, f1] = &self.faces;
        let diff = f0.v.sub(&f1.v, btr);
        for (g, d) in self.g.iter_mut().zip(&diff.a) {
            g.merge_computed(d, btr);
        }
        merge_v(&mut self.t, &f0.v.sub(&sys.leader, btr), btr);
        if let Some(g3) = self.g.get_mut(3) {
            g3.merge_computed(&self.t.dot(&sys.tangent, btr), btr);
        }
        self.valid |= all;
        let mv = |v: &VSer<T>| VSer {
            a: v.a.each_ref().map(|c| c.masked(tr)),
        };
        let [f0, f1] = &self.faces;
        Ok((
            self.g.each_ref().map(|c| c.masked(tr)),
            [mv(&f0.s), mv(&f1.s)],
            mv(&f0.v),
            [mv(&f0.n), mv(&f1.n)],
        ))
    }
}

/// `sm_GnTopJets` (3bc54a4): the section's jets of order `top` (its order n) and up, from
/// the faces' order-n partials only. The feet's parameters are solved to order n - 1 in
/// eps, and the face curve `C_i(tau) = S_i(x_i(tau))`, `x_i` so truncated, is a curve on
/// face i whose jet agrees with the feet's to order n - 1; its eps^n coefficient's part
/// along the face's normal is the face's own, its tangent part only the
/// parametrization's. The jet of order k >= n is that normal part, `N (N . C_k)`, so the
/// section is as continuous as the face's n-th partials. `f` holds C on entry, the jets on
/// return; `n` the faces' unit normals at the feet.
pub fn gn_top_jets<T: GnScalar>(top: usize, tr: Trunc, n: &[VSer<T>; 2], f: &mut [VSer<T>; 2]) {
    if top < 1 {
        return;
    }
    let sigma = Trunc::new(tr.s(), 0).unwrap_or(tr);
    for (ff, nf) in f.iter_mut().zip(n) {
        let mut sn = nf.clone();
        for comp in &mut sn.a {
            for i in 0..=MAX_S {
                for kk in 1..=MAX_E {
                    comp.set(i, kk, T::of(0.0));
                }
            }
        }
        for kk in top..=tr.e() {
            let mut sc = sn.clone();
            for (cc, fc) in sc.a.iter_mut().zip(&ff.a) {
                for i in 0..=MAX_S {
                    for k2 in 0..=MAX_E {
                        let v = if k2 == 0 && i <= tr.s() {
                            fc.at(i, kk)
                        } else {
                            T::of(0.0)
                        };
                        cc.set(i, k2, v);
                    }
                }
            }
            let sj = sn.times(&sn.dot(&sc, sigma), sigma);
            for (fc, jc) in ff.a.iter_mut().zip(&sj.a) {
                for i in 0..=tr.s() {
                    fc.set(i, kk, jc.at(i, 0));
                }
            }
        }
    }
}

/// `sm_GnJacobian`: the Jacobian `dG/dx` at x's constant terms and tau = 1: G along
/// `x + e_j eps`, first order. An equal setback's offset still follows the feet.
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
        let mut sx = core::array::from_fn(|_| Ser::constant(T::of(0.0)));
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
            return Err(SolidError::blend(
                OP,
                BlendFailure::SectionDegenerate(SectionDegeneracy::NormalsAgree),
            ));
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
    /// `sV`: V, the point at `tau r_i` from the faces (the ball's centre at `tau = 1`).
    pub v: VSer<f64>,
    /// `dW`: the equal area thumbweight there (NaN: not resolved).
    pub w: f64,
    /// `dWds`: and its s-derivative (NaN: not resolved).
    pub wds: f64,
    /// `lSteps`: the Newton steps the feet took (014f66f).
    pub steps: u32,
    /// `aiBefore` (3bc54a4): the series above are the station's on the pieces after it
    /// along s (the leader's, a face's where a foot is on one of its knot lines); a
    /// face's side for the pieces before it ([`sided_span`]).
    pub before: [i32; 4],
    /// `bSided`: the series on the pieces before it are set (`xl`, `fl`, `wdsl`).
    pub sided: bool,
    /// `asXL`: x on the pieces before the station, whose s-derivatives the span ending
    /// here takes (the values agree; the derivatives differ where the leader or a face is
    /// not smooth at the station).
    pub xl: [Ser<f64>; 4],
    /// `asFL`: the feet on the pieces before it.
    pub fl: [VSer<f64>; 2],
    /// `sVL` (cc08236): V on the pieces before it.
    pub vl: VSer<f64>,
    /// `dWdsL`: the equal area thumbweight's s-derivative on them.
    pub wdsl: f64,
}

impl GnStation {
    /// `sm_InF`: foot `f`'s series as the span ending here takes them.
    #[must_use]
    pub fn in_f(&self, f: usize) -> Option<&VSer<f64>> {
        if self.sided {
            self.fl.get(f)
        } else {
            self.f.get(f)
        }
    }

    /// `sm_InX`: parameter `q`'s series as the span ending here takes them.
    #[must_use]
    pub fn in_x(&self, q: usize) -> Option<&Ser<f64>> {
        if self.sided {
            self.xl.get(q)
        } else {
            self.x.get(q)
        }
    }

    /// `sm_InWds`: the thumbweight's s-derivative as the span ending here takes it.
    #[must_use]
    pub const fn in_wds(&self) -> f64 {
        if self.sided {
            self.wdsl
        } else {
            self.wds
        }
    }
}

/// The options of `sm_GnSolveStation` past its station (3bc54a4 and 014f66f): `pR`
/// (a radius law as a series in sigma), `lTop` (the section's order: its jets of that
/// order and up are the face curve's normal parts, [`gn_top_jets`]; 0 none), `pSides`
/// (a face's side at a knot line for each parameter, [`sided_span`]) and `bBefore` (the
/// leader's piece before `s` at one of its knots).
#[derive(Clone, Copy, Debug, Default)]
pub struct StationOptions<'a> {
    /// `pR`.
    pub law: Option<&'a Ser<f64>>,
    /// Equal setbacks measured at the current station, instead of a leader law.
    pub equal_setback: Option<f64>,
    /// `lTop`.
    pub top: usize,
    /// `pSides`.
    pub sides: Option<[i32; 4]>,
    /// `bBefore`.
    pub before: bool,
}

/// What `sm_GnSolveStation` hands back through `pC` and `pN`: the face curves C (the feet
/// before [`gn_top_jets`]) and the faces' unit normals at the feet.
#[derive(Clone, Copy, Debug)]
pub struct StationCurves {
    /// `pC`.
    pub c: [VSer<f64>; 2],
    /// `pN`.
    pub n: [VSer<f64>; 2],
}

/// `sm_LeaderSeries`: the leader (centre line) at `s` as series in sigma up to `tr.s`: its
/// point, and its unit tangent (which takes one derivative more).
///
/// # Errors
///
/// Those of the curve's evaluation; `SM_ERR` where its tangent vanishes.
pub fn leader_series(c: &BSplineCurve, s: f64, tr: Trunc) -> Result<(VSer<f64>, VSer<f64>)> {
    leader_series_sided(c, s, tr, false)
}

/// [`leader_series`] with its `bBefore` (3bc54a4): at a knot of the leader, its piece
/// before `s` (lower interval) instead of the one after it.
///
/// # Errors
///
/// As [`leader_series`].
pub fn leader_series_sided(
    c: &BSplineCurve,
    s: f64,
    tr: Trunc,
    before: bool,
) -> Result<(VSer<f64>, VSer<f64>)> {
    let sigma = Trunc::new(tr.s(), 0)?;
    // a polynomial leader's derivatives past its degree are zero; a rational one's are
    // evaluated (014f66f)
    let mut derivs = tr.s() + 1;
    if !c.is_rational()? {
        derivs = derivs.min(c.degree()?);
    }
    // "SmGnSectionFSG: the leader's derivatives of this order are not evaluated": a
    // rational leader's order can pass the most the curve evaluator takes
    if derivs > MAX_DERIV {
        return Err(SolidError::blend(
            OP,
            BlendFailure::SectionUnsupported(SectionLimit::OrderPastSeries {
                order: derivs,
                most: MAX_DERIV,
            }),
        ));
    }
    let mut d = c.evaluate(s, derivs, !before, false).map_err(|e| {
        Failure::new(
            OP,
            BlendFailure::Defect {
                why: "the leader's evaluation failed at an order within the most the curve \
                      evaluator takes, on a curve whose degree it just read",
            },
        )
        .caused_by(e)
        .into_error()
    })?;
    d.resize(tr.s() + 2, Vec3::ZERO);
    let zero = V3 {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };
    let mut l = VSer::constant(zero);
    let mut d1 = l;
    let mut fact = 1.0_f64;
    // (`d` holds the orders 0 to `tr.s() + 1`: each j and the next, j to `tr.s()`)
    for (j, (dj, dj1)) in d.iter().zip(d.iter().skip(1)).enumerate() {
        if j > 0 {
            fact *= j as f64;
        }
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
    gn_solve_station_counted(k, faces, leader, s, seed, r, side, tr, tol, None).0
}

/// [`gn_solve_station`] with the fork's `pR` (a radius law as a series in sigma, 014f66f)
/// and the Newton steps taken, which are counted even where the solve fails.
#[allow(clippy::too_many_arguments)]
pub fn gn_solve_station_counted(
    k: &Kernel,
    faces: [&Surface; 2],
    leader: &BSplineCurve,
    s: f64,
    seed: [f64; 4],
    r: [f64; 2],
    side: [f64; 2],
    tr: Trunc,
    tol: f64,
    law: Option<&Ser<f64>>,
) -> (Result<GnStation>, u32) {
    let opts = StationOptions {
        law,
        ..StationOptions::default()
    };
    let (st, steps) = gn_solve_station_with(k, faces, leader, s, seed, r, side, tr, tol, opts);
    (st.map(|(st, _)| st), steps)
}

/// `sm_GnSolveStation` with all its options ([`StationOptions`]), its `pC` and `pN`
/// ([`StationCurves`]) and the Newton steps taken (counted even where the solve fails).
#[allow(clippy::too_many_arguments)]
pub fn gn_solve_station_with(
    k: &Kernel,
    faces: [&Surface; 2],
    leader: &BSplineCurve,
    s: f64,
    seed: [f64; 4],
    r: [f64; 2],
    side: [f64; 2],
    tr: Trunc,
    tol: f64,
    opts: StationOptions<'_>,
) -> (Result<(GnStation, StationCurves)>, u32) {
    let mut steps = 0_u32;
    let st = solve_station_body(
        k, faces, leader, s, seed, r, side, tr, tol, opts, &mut steps,
    );
    (st, steps)
}

/// The body of [`gn_solve_station_counted`].
#[allow(clippy::too_many_arguments)]
fn solve_station_body(
    k: &Kernel,
    faces: [&Surface; 2],
    leader: &BSplineCurve,
    s: f64,
    seed: [f64; 4],
    r: [f64; 2],
    side: [f64; 2],
    tr: Trunc,
    tol: f64,
    opts: StationOptions<'_>,
    steps: &mut u32,
) -> Result<(GnStation, StationCurves)> {
    let law = opts.law;
    let equal_setback = opts.equal_setback;
    let sides = opts.sides.unwrap_or([0, 0, 0, 0]);
    if tr.s() > MAX_S {
        return Err(SolidError::blend(
            OP,
            BlendFailure::SectionUnsupported(SectionLimit::OrderPastSeries {
                order: tr.s(),
                most: MAX_S,
            }),
        ));
    }
    let lk = tr.s() + tr.e() + 1;
    let (l, t) = leader_series_sided(leader, s, tr, opts.before)?;
    let [f0, f1] = faces;
    // (a side names a piece at the seed, on the knot line: every iterate keeps it)
    let partials_at = |x: &[f64; 4], order: usize| -> Result<[Partials<f64>; 2]> {
        Ok([
            surface_partials_pinned(
                k,
                f0,
                Point2::new(x[0], x[1]),
                Point2::new(seed[0], seed[1]),
                order,
                [sides[0], sides[1]],
            )?,
            surface_partials_pinned(
                k,
                f1,
                Point2::new(x[2], x[3]),
                Point2::new(seed[2], seed[3]),
                order,
                [sides[2], sides[3]],
            )?,
        ])
    };

    // the feet at tau = 1, by Newton (the leader at sigma = 0 only)
    let mut x = seed;
    let point = Trunc::new(0, 0)?;
    let mut converged = false;
    let mut it = 0;
    // the last iteration's residual and step, for a station that does not converge
    let mut last: Option<(f64, f64)> = None;
    while it < 50 && !converged {
        *steps += 1;
        let pp = partials_at(&x, 2)?;
        let [p0, p1] = &pp;
        let sys = GnSystem {
            p: [p0, p1],
            leader: l,
            tangent: t,
            r,
            side,
            hold: false,
            law,
            equal_setback,
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
        if res <= tol && step <= STATION_STEP {
            converged = true;
        }
        last = Some((res, step));
        it += 1;
    }
    if !converged {
        return Err(SolidError::blend(
            OP,
            BlendFailure::StationDidNotConverge {
                s,
                residual: last.map(|(res, _)| res),
                tolerance: tol,
                step: last.map(|(_, step)| step),
                iterations: u32::try_from(it).ok(),
            },
        ));
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
        law,
        equal_setback,
    };
    let mut sx = x.map(Ser::constant);
    let jac = gn_jacobian(&sys, &sx)?;
    // (the coefficients of one total degree do not enter each other's: one evaluation
    // solves them all; G is computed only at that degree and below, and at the powers of
    // eps solved for, bit for bit the whole truncation's there (`Trunc::computing`), and
    // not at all at an order with none to solve)
    let eps_solved = if opts.top >= 1 {
        tr.e().min(opts.top - 1)
    } else {
        tr.e()
    };
    for tot in 1..=(tr.s() + tr.e()) {
        let eps = eps_solved.min(tot);
        if tot > tr.s() + eps {
            continue;
        }
        let (g, _) = gn_evaluate(&sys, &sx, tr.computing(tot, eps))?;
        for i in 0..=tr.s().min(tot) {
            let kk = tot - i;
            // (to order lTop - 1 in eps with lTop: gn_top_jets)
            if kk > tr.e() || (opts.top >= 1 && kk >= opts.top) {
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
    let (_, mut f, v, n) = gn_evaluate_vn(&sys, &sx, tr)?;
    let curves = StationCurves { c: f, n };
    gn_top_jets(opts.top, tr, &n, &mut f);
    let zero = V3 {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };
    Ok((
        GnStation {
            s,
            x: sx,
            f,
            v,
            w: f64::NAN,
            wds: f64::NAN,
            steps: *steps,
            before: [0; 4],
            sided: false,
            xl: [Ser::constant(0.0); 4],
            fl: [VSer::constant(zero); 2],
            vl: VSer::constant(zero),
            wdsl: f64::NAN,
        },
        curves,
    ))
}

/// `sm_FeetOnKnots` (c5b6bb7): the feet `x` of a station that stand on a knot line of
/// their face (within the margin a crossing station is placed by, `Build`'s), moving
/// along s as `dir` says, where the face's partials to order `partials` differ on the
/// line's two sides (it is `C^(p - multiplicity)` there): the side of that line each foot
/// coordinate is taken on (+-1, [`sided_span`]'s; 0 off such a line), the piece it moves
/// into as s grows, or with `before` the one it comes from; and whether some foot's side
/// is not the piece a solve without sides takes there (the span the foot's value is in,
/// [`span`]'s), so that the station is solved again with these sides.
///
/// # Errors
///
/// Those of a face's NURB and its knots.
pub fn feet_on_knots(
    k: &Kernel,
    faces: [&Surface; 2],
    x: [f64; 4],
    dir: [f64; 4],
    before: bool,
    partials: usize,
) -> Result<([i32; 4], bool)> {
    let mut sides = [0_i32; 4];
    let mut other = false;
    for (f, face) in faces.into_iter().enumerate() {
        // SM_CAST_PTR(SmBSplineSurface, apFace[f]) (a blend by its definition: its net)
        let Some(bs) = as_b_spline(k, face)? else {
            continue;
        };
        for c in 0..2 {
            let param = if c == 0 {
                SurfParamType::U
            } else {
                SurfParamType::V
            };
            // GetKnotsPointers' full knot vector
            let (knots, mult) = bs.knots(param, None)?;
            let kv = full_knots(&knots, &mult);
            let ord = bs.degree(param) + 1;
            let q = 2 * f + c;
            let xq = x.get(q).copied().unwrap_or(f64::NAN);
            let mut m = ord;
            while m + ord < kv.len() {
                let km = knot(&kv, m as isize);
                if (xq - km).abs() <= 1.0e-9 * (1.0 + km.abs()) {
                    // the face is C^(p - multiplicity) across the line: partials to that
                    // order agree on its sides
                    let mult_m = kv.iter().filter(|&&kj| kj == km).count();
                    let smooth = (partials as i64) <= (ord as i64 - 1) - mult_m as i64;
                    if smooth {
                        break;
                    }
                    let d = dir.get(q).copied().unwrap_or(f64::NAN);
                    let mut side = if d < 0.0 { -1 } else { 1 };
                    if before {
                        side = -side;
                    }
                    if let Some(sq) = sides.get_mut(q) {
                        *sq = side;
                    }
                    if (side < 0 && xq >= km) || (side > 0 && xq < km) {
                        other = true;
                    }
                    break;
                }
                m += 1;
            }
        }
    }
    Ok((sides, other))
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
    fn equal_setback_uses_the_oriented_wedge_and_its_derivative() {
        let tr = Trunc::new(1, 0).unwrap();
        let first = VSer::constant(V3 {
            x: 1.0,
            y: 0.0,
            z: 0.0,
        });
        let root3 = 3.0_f64.sqrt();
        let mut second = VSer::constant(V3 {
            x: 0.5,
            y: 0.5 * root3,
            z: 0.0,
        });
        let [cosine, sine, _] = &mut second.a;
        cosine.set(1, 0, 1.0);
        sine.set(1, 0, -1.0 / root3);
        let d = 0.2;
        let radius = equal_setback_radius(&[first, second], [1.0, -1.0], d, tr).unwrap();
        // The signed cosine is -1/2. The radius is d/sqrt(3), and
        // differentiating its squared constraint gives dr/dq = r/(1-q^2).
        let expected = d / root3;
        assert!((radius.at(0, 0) - expected).abs() < 1.0e-15);
        assert!((radius.at(1, 0) + 4.0 * expected / 3.0).abs() < 1.0e-15);
    }

    #[test]
    fn equal_setback_encloses_the_radius_only_inside_its_wedge_domain() {
        let tr = Trunc::new(0, 0).unwrap();
        let normal = |x, y| {
            VSer::constant(V3 {
                x,
                y,
                z: Ivl::from(0.0),
            })
        };
        let first = normal(Ivl::from(1.0), Ivl::from(0.0));
        let second = normal(Ivl::new(0.4, 0.6), Ivl::new(0.8, 0.92));
        let d = 0.2;
        let radius = equal_setback_radius(&[first.clone(), second], [1.0, 1.0], d, tr)
            .unwrap()
            .at(0, 0);
        assert!(radius.lo <= d * (7.0_f64 / 3.0).sqrt());
        assert!(radius.hi >= 2.0 * d);
        // A box reaching the degenerate wedge cannot prove a radius there.
        let second = normal(Ivl::new(0.9, 1.0), Ivl::new(0.0, 0.44));
        assert!(equal_setback_radius(&[first, second], [1.0, 1.0], d, tr).is_err());
    }

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

    /// `sm_SidedSpan` (3bc54a4): on a knot line the piece before it or after it, the
    /// span `sm_Span` finds elsewhere (a cubic knot vector with a double inner knot at 1).
    #[test]
    fn a_sided_span_takes_the_piece_on_its_side() {
        let u = [0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 2.0, 2.0, 2.0, 2.0];
        let (p, n) = (3, 6);
        assert_eq!(span(&u, p, n, 1.0), 5);
        assert_eq!(sided_span(&u, p, n, 1.0, 0), 5);
        assert_eq!(sided_span(&u, p, n, 1.0, 1), 5);
        assert_eq!(sided_span(&u, p, n, 1.0, -1), 3);
        assert_eq!(sided_span(&u, p, n, 1.0 + 1e-10, -1), 3);
        assert_eq!(sided_span(&u, p, n, 1.0 - 1e-10, 1), 5);
        assert_eq!(sided_span(&u, p, n, 0.5, -1), 3);
        assert_eq!(sided_span(&u, p, n, 1.5, 1), 5);
    }

    /// The double next to `x` towards +inf (`x` finite).
    fn next_up(x: f64) -> f64 {
        if x == 0.0 {
            f64::from_bits(1)
        } else if x > 0.0 {
            f64::from_bits(x.to_bits() + 1)
        } else {
            f64::from_bits(x.to_bits() - 1)
        }
    }

    fn next_down(x: f64) -> f64 {
        -next_up(-x)
    }

    /// The exact result `x` of an operation as its double `s` and the sign of `x - s`:
    /// the double at or below it, and the one at or above it.
    fn bracket(s: f64, sign: f64) -> (f64, f64) {
        if sign > 0.0 {
            (s, next_up(s))
        } else if sign < 0.0 {
            (next_down(s), s)
        } else {
            (s, s)
        }
    }

    fn sum(a: f64, b: f64) -> (f64, f64) {
        let s = a + b;
        let bb = s - a;
        bracket(s, (a - (s - bb)) + (b - bb))
    }

    fn product(a: f64, b: f64) -> (f64, f64) {
        let p = a * b;
        bracket(p, a.mul_add(b, -p))
    }

    fn quotient(a: f64, b: f64) -> (f64, f64) {
        let q = a / b;
        // a - q b exactly, and a / b - q = (a - q b) / b
        let r = (-q).mul_add(b, a);
        bracket(q, r * b.signum())
    }

    /// Intervals with the tightest outward rounding there is (IEEE 754 directed
    /// rounding): each end is the double next to the exact result on its side, or the
    /// result itself when it is a double. An enclosure that holds under it holds by
    /// construction, not by spare width.
    #[derive(Clone, Copy, Debug, PartialEq)]
    struct Directed {
        lo: f64,
        hi: f64,
    }

    impl Directed {
        fn hull(c: [(f64, f64); 4]) -> Self {
            let lo = c.iter().map(|x| x.0).fold(f64::INFINITY, f64::min);
            let hi = c.iter().map(|x| x.1).fold(f64::NEG_INFINITY, f64::max);
            Self { lo, hi }
        }
    }

    impl core::ops::Add for Directed {
        type Output = Self;
        fn add(self, b: Self) -> Self {
            Self {
                lo: sum(self.lo, b.lo).0,
                hi: sum(self.hi, b.hi).1,
            }
        }
    }

    impl core::ops::Sub for Directed {
        type Output = Self;
        fn sub(self, b: Self) -> Self {
            Self {
                lo: sum(self.lo, -b.hi).0,
                hi: sum(self.hi, -b.lo).1,
            }
        }
    }

    impl core::ops::Mul for Directed {
        type Output = Self;
        fn mul(self, b: Self) -> Self {
            let (a, c) = (self, b);
            Self::hull([
                product(a.lo, c.lo),
                product(a.lo, c.hi),
                product(a.hi, c.lo),
                product(a.hi, c.hi),
            ])
        }
    }

    impl core::ops::Div for Directed {
        type Output = Self;
        fn div(self, b: Self) -> Self {
            assert!(b.lo > 0.0 || b.hi < 0.0, "a divisor holding 0");
            let a = self;
            Self::hull([
                quotient(a.lo, b.lo),
                quotient(a.lo, b.hi),
                quotient(a.hi, b.lo),
                quotient(a.hi, b.hi),
            ])
        }
    }

    impl GnScalar for Directed {
        type Storage = [[Self; STORE_E + 1]; MAX_S + 1];
        fn of(d: f64) -> Self {
            Self { lo: d, hi: d }
        }
        fn is_positive(self) -> bool {
            self.lo > 0.0
        }
        fn sqrt(self) -> Self {
            // sqrt is correctly rounded: a step outward holds the exact root
            Self {
                lo: next_down(self.lo.max(0.0).sqrt()),
                hi: next_up(self.hi.sqrt()),
            }
        }
    }

    /// An exact dyadic rational `m 2^e`: every double is one, and sums and products of a
    /// few doubles near 1 stay one in an `i128`.
    #[derive(Clone, Copy, Debug)]
    struct Dyadic {
        m: i128,
        e: i32,
    }

    impl Dyadic {
        fn of(x: f64) -> Self {
            assert!(x.is_finite());
            let bits = x.to_bits();
            let biased = ((bits >> 52) & 0x7ff) as i32;
            let frac = i128::from(bits & ((1_u64 << 52) - 1));
            let (m, e) = if biased == 0 {
                (frac, -1074)
            } else {
                (frac | (1_i128 << 52), biased - 1075)
            };
            if m == 0 {
                return Self { m: 0, e: 0 };
            }
            // odd m: the least exponent for the number
            let z = m.trailing_zeros();
            Self {
                m: if bits >> 63 == 1 { -(m >> z) } else { m >> z },
                e: e + z as i32,
            }
        }

        /// `m 2^(self.e - e)`, for `e <= self.e`.
        fn scaled(self, e: i32) -> i128 {
            if self.m == 0 {
                return 0;
            }
            let s = u32::try_from(self.e - e).unwrap();
            assert!(s < 126, "out of the i128 range");
            self.m.checked_mul(1_i128 << s).unwrap()
        }

        fn sub(self, b: Self) -> Self {
            let e = match (self.m, b.m) {
                (0, _) => b.e,
                (_, 0) => self.e,
                _ => self.e.min(b.e),
            };
            Self {
                m: self.scaled(e).checked_sub(b.scaled(e)).unwrap(),
                e,
            }
        }

        fn mul(self, b: Self) -> Self {
            Self {
                m: self.m.checked_mul(b.m).unwrap(),
                e: self.e + b.e,
            }
        }
    }

    /// `[lo, hi]` holds `n / d` (`d > 0`) exactly: `lo d <= n <= hi d`.
    fn holds(lo: f64, hi: f64, n: Dyadic, d: Dyadic) -> bool {
        assert!(d.m > 0);
        n.sub(Dyadic::of(lo).mul(d)).m >= 0 && Dyadic::of(hi).mul(d).sub(n).m >= 0
    }

    /// #2559: the basis' knot differences are intervals rounded outward from the knots,
    /// not the rounded double difference as a point. On `[a, a, b, b]` the linear basis at
    /// `x` is `(b - x) / D` and `(x - a) / D`, its derivatives `-1 / D` and `1 / D`, with
    /// `D = b - a` exactly. With `a = 0.7`, `b = 3.47`, `fl(b - a)` is 2.2e-16 above `D`,
    /// and the reciprocal step of a tight interval division from it misses `1 / D`.
    /// Under the tightest outward rounding every enclosure holds the exact value (checked
    /// in exact rational arithmetic), so the crate's `Ivl`, which rounds wider, holds it by
    /// construction too.
    #[test]
    fn the_basis_encloses_its_exact_values_under_the_tightest_rounding() {
        fn check<T: GnScalar>(a: f64, b: f64, x: f64, ends: impl Fn(T) -> (f64, f64)) {
            let u = [a, a, b, b];
            let k = span(&u, 1, 2, x);
            assert_eq!(k, 1);
            let n = ders_basis(&u, k, 1, T::of(x), 1);
            let d = Dyadic::of(b).sub(Dyadic::of(a));
            let one = Dyadic::of(1.0);
            let want = [
                [
                    Dyadic::of(b).sub(Dyadic::of(x)),
                    Dyadic::of(x).sub(Dyadic::of(a)),
                ],
                [Dyadic::of(0.0).sub(one), one],
            ];
            for (r, (row, want_row)) in n.iter().zip(&want).enumerate() {
                for (j, (&v, &w)) in row.iter().zip(want_row).enumerate() {
                    let (lo, hi) = ends(v);
                    assert!(
                        holds(lo, hi, w, d),
                        "a = {a:?}, b = {b:?}, x = {x:?}: N^({r})_{j} = [{lo:?}, {hi:?}]"
                    );
                }
            }
        }
        // the issue's knots first, then knots in [1/4, 2) a span of [0.1, 3.1) apart
        let mut cases = vec![(0.7, 3.47)];
        let mut state = 0x2559_u64;
        let mut unit = || {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (state >> 11) as f64 / (1_u64 << 53) as f64
        };
        for _ in 0..2000 {
            let a = 0.25 + 1.75 * unit();
            cases.push((a, a + 0.1 + 3.0 * unit()));
        }
        let mut inexact = 0;
        for (a, b) in cases {
            if Dyadic::of(b - a).sub(Dyadic::of(b).sub(Dyadic::of(a))).m != 0 {
                inexact += 1;
            }
            let x = 0.5 * (a + b);
            check::<Directed>(a, b, x, |v| (v.lo, v.hi));
            check::<Ivl>(a, b, x, |v| (v.lo, v.hi));
        }
        // 554 of the 2001 differences are not doubles: the case the construction is for
        assert_eq!(inexact, 554);
    }

    /// [`GnIncremental`] fed as the feet's Taylor models feed it (x filled in order by
    /// order, each order evaluated with the lower ones' coefficients set and its own 0,
    /// then one whole evaluation), against [`gn_evaluate_vn`] at each call: equal bit for
    /// bit. Returns the positions it computed and those the whole evaluations compute, and
    /// whether each face's offset (by `r`) is the joint expansion ([`offset_near_focal`]).
    fn incremental_matches_whole<T: GnScalar + PartialEq + core::fmt::Debug>(
        mk: impl Fn(f64) -> T,
        s: usize,
        e: usize,
        n: usize,
        law: bool,
        hold: bool,
        r: [f64; 2],
    ) -> (u64, u64, [bool; 2]) {
        let mut seed = 0x9e37_79b9_7f4a_7c15_u64;
        let mut next = move || {
            seed = seed
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (seed >> 11) as f64 / (1_u64 << 53) as f64 - 0.5
        };
        let mut v3 = |base: [f64; 3]| V3 {
            x: mk(base[0] + 0.3 * next()),
            y: mk(base[1] + 0.3 * next()),
            z: mk(base[2] + 0.3 * next()),
        };
        let mut faces = [Partials::default(), Partials::default()];
        for (f, pf) in faces.iter_mut().enumerate() {
            let sgn = if f == 0 { 1.0 } else { -1.0 };
            for a in 0..=MAX_K {
                for b in 0..=(MAX_K - a) {
                    let base = match (a, b) {
                        (1, 0) => [1.0, 0.0, 0.0],
                        (0, 1) => [0.0, sgn, 0.2],
                        _ => [0.0; 3],
                    };
                    pf.set(a, b, v3(base));
                }
            }
        }
        let mut ser = |c0: f64| {
            let mut x = Ser::constant(mk(c0));
            for i in 0..=MAX_S {
                for k in 0..=MAX_E {
                    if i + k > 0 {
                        x.set(i, k, mk(next()));
                    }
                }
            }
            x
        };
        let leader = VSer {
            a: [ser(0.1), ser(0.2), ser(0.3)],
        };
        let tangent = VSer {
            a: [ser(0.0), ser(0.6), ser(0.8)],
        };
        let law_ser = ser(0.7);
        let [p0, p1] = &faces;
        let sys = GnSystem {
            p: [p0, p1],
            leader,
            tangent,
            r,
            side: [1.0, -1.0],
            hold,
            law: law.then_some(&law_ser),
            equal_setback: None,
        };
        let near = [
            offset_near_focal(p0, offset_d0(&sys, r[0], 1.0)),
            offset_near_focal(p1, offset_d0(&sys, r[1], -1.0)),
        ];
        let tr = Trunc::new(s, e).unwrap();
        let mut sx = [0.3, -0.2, 0.1, 0.4].map(|c| Ser::constant(mk(c)));
        let mut inc = GnIncremental::new(&sys);
        let mut whole = 0_u64;
        let eps_solved = if n >= 1 { e.min(n - 1) } else { e };
        for tot in 1..=(s + e) {
            let eps = eps_solved.min(tot);
            if tot > s + eps {
                continue;
            }
            let at = tr.computing(tot, eps);
            let want = gn_evaluate_vn(&sys, &sx, at).unwrap();
            whole += u64::from(at.computed_mask().count_ones());
            assert_eq!(inc.evaluate(&sx, at).unwrap(), want, "order {tot}");
            for i in 0..=s.min(tot) {
                let kk = tot - i;
                if kk > e || (n >= 1 && kk >= n) {
                    continue;
                }
                for xc in &mut sx {
                    xc.set(i, kk, mk(0.2 * next()));
                }
            }
        }
        let want = gn_evaluate_vn(&sys, &sx, tr).unwrap();
        whole += u64::from(tr.computed_mask().count_ones());
        assert_eq!(inc.evaluate(&sx, tr).unwrap(), want, "whole");
        // nothing changed: nothing to compute, the same values
        let before = inc.computed();
        assert_eq!(inc.evaluate(&sx, tr).unwrap(), want, "again");
        assert_eq!(inc.computed(), before);
        // a changed lower coefficient: computed again from it
        sx[1].set(1, 0, mk(0.05));
        assert_eq!(
            inc.evaluate(&sx, tr).unwrap(),
            gn_evaluate_vn(&sys, &sx, tr).unwrap()
        );
        (before, whole, near)
    }

    #[test]
    fn incremental_evaluation_is_the_whole_one_bit_for_bit() {
        let tm = |v: f64| {
            crate::blend::fillet_section::feet_tm::Ft::from_points(&[v, 0.01 * v, -0.005 * v])
        };
        // the enclosures' offsets taken both ways: as the sum (far from a focal distance)
        // and as the joint expansion (#19306)
        let mut seen = [false; 2];
        for (s, e, n) in [(3, 2, 2), (2, 3, 2), (3, 0, 0), (2, 2, 0), (4, 1, 1)] {
            for (law, hold) in [(false, false), (true, false), (false, true)] {
                for r in [[0.5, 0.75], [8.0, 12.0]] {
                    incremental_matches_whole(|v| v, s, e, n, law, hold, r);
                    let (_, _, ni) = incremental_matches_whole(Ivl::from, s, e, n, law, hold, r);
                    let (_, _, nt) = incremental_matches_whole(tm, s, e, n, law, hold, r);
                    for near in ni.into_iter().chain(nt) {
                        if let Some(x) = seen.get_mut(usize::from(near)) {
                            *x = true;
                        }
                    }
                }
            }
        }
        assert_eq!(seen, [true, true]);
    }

    /// The work counter of the feet's Taylor models: their order-by-order evaluations
    /// compute each coefficient of G at most twice (at its order with its own 0, and once
    /// it is set), not again at every later order.
    #[test]
    fn incremental_evaluation_computes_each_order_once() {
        // the certificate's orders: order 3 in sigma, 2 in eps, the jets of order 2 the
        // faces'
        let (inc, whole, _) = incremental_matches_whole(|v| v, 3, 2, 2, false, false, [0.5, 0.75]);
        let full = u64::from(Trunc::new(3, 2).unwrap().computed_mask().count_ones());
        assert_eq!(full, 12);
        assert!(
            inc <= 2 * full,
            "{inc} computed, at most twice each of {full}"
        );
        assert_eq!((inc, whole), (19, 35));
    }

    #[test]
    fn an_order_past_the_series_is_a_typed_failure() {
        let k = Kernel::default();
        let s = plane(Vec3::X, Vec3::Y);
        let error = surface_partials(&k, &s, Point2::new(0.5, 0.5), MAX_K + 1).unwrap_err();
        assert_eq!(
            error,
            SolidError::blend(
                OP,
                BlendFailure::SectionUnsupported(SectionLimit::OrderPastSeries {
                    order: MAX_K + 1,
                    most: MAX_K
                })
            )
        );
        assert_eq!(error.upstream_status(), Some(crate::error::SM_ERR));
    }

    /// A cylinder of radius 10 about z (height 20) as its NURBS, a point of it inside a
    /// span, and its spans there.
    fn cylinder_at() -> (Nurbs, f64, f64, usize, usize) {
        let k = Kernel::default();
        let mut cyl = crate::surface::cone::Cone::new_cylinder_bounded(
            Vec3::ZERO,
            Vec3::X,
            Vec3::Y,
            10.0,
            0.0,
            360.0,
            20.0,
            false,
            false,
            false,
        );
        cyl.make_nurb(&k).unwrap();
        let s = Surface::from(cyl);
        let n = Nurbs::of(&as_b_spline(&k, &s).unwrap().unwrap()).unwrap();
        let (u0, u1) = (knot(&n.u, n.p as isize), knot(&n.u, n.nu as isize));
        let (v0, v1) = (knot(&n.v, n.q as isize), knot(&n.v, n.nv as isize));
        let (u, v) = (u0 + 0.137 * (u1 - u0), 0.5 * (v0 + v1));
        let (ku, kv) = (span(&n.u, n.p, n.nu, u), span(&n.v, n.q, n.nv, v));
        (n, u, v, ku, kv)
    }

    /// The offset `S + d0 N` and its unit normal at `(u, v)` of `n`, in doubles.
    fn offset_point(n: &Nurbs, u: f64, v: f64, d0: f64) -> [f64; 3] {
        let ku = span(&n.u, n.p, n.nu, u);
        let kv = span(&n.v, n.q, n.nv, v);
        let p = nurbs_partials::<f64>(n, ku, kv, u, v, 1);
        let (s, su, sv) = (p.at(0, 0), p.at(1, 0), p.at(0, 1));
        let c = [
            su.y * sv.z - su.z * sv.y,
            su.z * sv.x - su.x * sv.z,
            su.x * sv.y - su.y * sv.x,
        ];
        let l = (c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).sqrt();
        [
            s.x + d0 * c[0] / l,
            s.y + d0 * c[1] / l,
            s.z + d0 * c[2] / l,
        ]
    }

    /// The offset point's joint expansion ([`offset_taylor`], #16652) is the offset's
    /// Taylor polynomial: on a cylinder of radius 10 offset by -9.99 and by +9.99 (a
    /// cylinder of radius 0.01 or 19.99 about the same axis, the cylinder scaled about it:
    /// every coefficient to degree 6 is the face's own scaled by 0.001 or 1.999 in x and y,
    /// the closed form), the polynomial of degree 6 at
    /// a step of a thousandth of the parameters' range (its remainder, of degree 7, below
    /// 1e-15 there) agrees with `S + d0 N` taken at the
    /// stepped point to 1e-11, lies at the closed-form radius, and the interval expansion
    /// at the same point (thin partials) holds the double one's coefficients to 1e-12.
    #[test]
    fn the_joint_offset_expansion_is_the_offset_point() {
        let (n, u, v, ku, kv) = cylinder_at();
        let deg = 6;
        let pf = nurbs_partials::<f64>(&n, ku, kv, u, v, deg + 1);
        let pi = nurbs_partials::<Ivl>(&n, ku, kv, Ivl::from(u), Ivl::from(v), deg + 1);
        let span_u = knot(&n.u, n.nu as isize) - knot(&n.u, n.p as isize);
        let span_v = knot(&n.v, n.nv as isize) - knot(&n.v, n.q as isize);
        for d0 in [-9.99, 9.99] {
            let w = offset_taylor(&pf, d0, deg).unwrap();
            let wi = offset_taylor(&pi, Ivl::from(d0), deg).unwrap();
            // closed form, every order: the offset cylinder is the cylinder scaled about
            // its axis (z) by k = 0.001 or 1.999, an affine map, so each Taylor
            // coefficient is the face's own, scaled by k in x and y
            let w00 = offset_at(&w, deg, 0, 0);
            let p00 = pf.at(0, 0);
            let kk = w00.x.hypot(w00.y) / p00.x.hypot(p00.y);
            assert!(
                (kk - 0.001).abs() < 1e-12 || (kk - 1.999).abs() < 1e-12,
                "d0 {d0}: k {kk}"
            );
            let mut fa = 1.0_f64;
            for a in 0..=deg {
                if a > 0 {
                    fa *= a as f64;
                }
                let mut fb = 1.0_f64;
                for b in 0..=(deg - a) {
                    if b > 0 {
                        fb *= b as f64;
                    }
                    let (c, p) = (offset_at(&w, deg, a, b), pf.at(a, b));
                    let want = [kk * p.x / (fa * fb), kk * p.y / (fa * fb), p.z / (fa * fb)];
                    let scale = 1.0 + p.x.abs().max(p.y.abs()).max(p.z.abs()) / (fa * fb);
                    for (x, y) in [c.x, c.y, c.z].iter().zip(want) {
                        assert!(
                            (x - y).abs() < 1e-10 * scale,
                            "d0 {d0} W[{a}][{b}]: {x} against {y}"
                        );
                    }
                }
            }
            for a in 0..=deg {
                for b in 0..=(deg - a) {
                    let (c, ci) = (offset_at(&w, deg, a, b), offset_at(&wi, deg, a, b));
                    for (x, xi) in [(c.x, ci.x), (c.y, ci.y), (c.z, ci.z)] {
                        let slack = 1e-12 * (1.0 + x.abs());
                        assert!(
                            xi.lo - slack <= x && x <= xi.hi + slack,
                            "d0 {d0} W[{a}][{b}] {x} not in {xi:?}"
                        );
                    }
                }
            }
            for (fu, fv) in [(0.001, 0.0), (0.0, 0.001), (-0.0007, 0.0004)] {
                let (du, dv) = (fu * span_u, fv * span_v);
                let mut p = [0.0_f64; 3];
                for a in 0..=deg {
                    for b in 0..=(deg - a) {
                        let c = offset_at(&w, deg, a, b);
                        let m = du.powi(a as i32) * dv.powi(b as i32);
                        p[0] += c.x * m;
                        p[1] += c.y * m;
                        p[2] += c.z * m;
                    }
                }
                let q = offset_point(&n, u + du, v + dv, d0);
                for (pc, qc) in p.iter().zip(&q) {
                    assert!((pc - qc).abs() < 1e-11, "d0 {d0}: {p:?} against {q:?}");
                }
                let radius = p[0].hypot(p[1]);
                let near = |r: f64| (radius - r).abs() < 1e-11;
                assert!(near(0.01) || near(19.99), "d0 {d0}: radius {radius}");
            }
        }
    }

    /// #19306: an enclosure takes the offset point as one expansion only where the offset is
    /// not proved short of a focal distance by [`FOCAL_REACH`], `d0 k <= 1/4` for both
    /// principal curvatures `k` at every point its partials enclose. On
    /// `z = (a u^2 + b v^2) / 2` at its origin (principal curvatures `a` and `b` along +z,
    /// the focal distances `1 / a` and `1 / b`), in intervals and in Taylor models; over a
    /// box of curvatures; for a NaN; on the cylinder of radius 10 as its rational net; and
    /// never for the floating-point jets, which keep the sum.
    #[test]
    fn the_offset_is_one_expansion_only_near_a_focal_distance() {
        fn quadric<T: GnScalar>(mk: impl Fn(f64) -> T, a: T, b: T) -> Partials<T> {
            let v = |x: f64, y: f64| V3 {
                x: mk(x),
                y: mk(y),
                z: mk(0.0),
            };
            let mut p = Partials::default();
            p.set(1, 0, v(1.0, 0.0));
            p.set(0, 1, v(0.0, 1.0));
            p.set(
                2,
                0,
                V3 {
                    z: a,
                    ..v(0.0, 0.0)
                },
            );
            p.set(
                0,
                2,
                V3 {
                    z: b,
                    ..v(0.0, 0.0)
                },
            );
            p
        }
        let tm = <crate::blend::fillet_section::feet_tm::Ft as GnScalar>::of;
        // (a, b, d0, near): near where d0 a or d0 b passes 1/4
        let cases = [
            (1.0, 0.25, 0.2, false),
            (1.0, 0.25, 0.3, true),
            (1.0, 0.25, -3.0, false),
            (1.0, -1.0, 0.2, false),
            (1.0, -1.0, -0.2, false),
            (1.0, -1.0, 0.3, true),
            (1.0, -1.0, -0.3, true),
            // past both focal distances: det(I - 4 d0 W) > 0, but not definite
            (1.0, 1.0, 2.5, true),
            // a plane has none
            (0.0, 0.0, 1.0e6, false),
        ];
        for (a, b, d0, near) in cases {
            let pi = quadric(Ivl::from, Ivl::from(a), Ivl::from(b));
            assert_eq!(offset_near_focal(&pi, Ivl::from(d0)), near, "{a} {b} {d0}");
            let pt = quadric(tm, tm(a), tm(b));
            assert_eq!(offset_near_focal(&pt, tm(d0)), near, "{a} {b} {d0}");
            assert!(
                !offset_near_focal(&quadric(|x| x, a, b), d0),
                "{a} {b} {d0}"
            );
        }
        // over a box of curvatures a in [0.8, 1.2]: 0.2 is short of a quarter of the way for
        // every one, 0.22 only for some (the middle's 1 among them)
        let pb = quadric(Ivl::from, Ivl::new(0.8, 1.2), Ivl::from(0.25));
        assert!(!offset_near_focal(&pb, Ivl::from(0.2)));
        assert!(offset_near_focal(&pb, Ivl::from(0.22)));
        let pi = quadric(Ivl::from, Ivl::from(1.0), Ivl::from(0.25));
        assert!(offset_near_focal(&pi, Ivl::from(f64::NAN)));
        // the cylinder of radius 10: toward its axis, near past 2.5; away from it, never
        let (n, u, v, ku, kv) = cylinder_at();
        let pc = nurbs_partials::<Ivl>(&n, ku, kv, Ivl::from(u), Ivl::from(v), 2);
        let pf = nurbs_partials::<f64>(&n, ku, kv, u, v, 1);
        let (s, su, sv) = (pf.at(0, 0), pf.at(1, 0), pf.at(0, 1));
        let outward = (su.y * sv.z - su.z * sv.y) * s.x + (su.z * sv.x - su.x * sv.z) * s.y;
        let toward = if outward > 0.0 { -1.0 } else { 1.0 };
        for (d, near) in [(2.4, false), (2.6, true), (9.99, true)] {
            assert_eq!(offset_near_focal(&pc, Ivl::from(d * toward)), near, "{d}");
            assert!(!offset_near_focal(&pc, Ivl::from(-d * toward)), "{d}");
        }
    }

    /// The root cause of #16652: near a face's focal distance the offset point moves far
    /// slower than the face's point (a cylinder of radius 10 offset by 9.99 toward its axis:
    /// a thousandth as fast), and the offset's enclosure must keep that. A foot whose
    /// first s-coefficient is an interval of half width w gives the offset's first
    /// coefficient a width of `2 w |W_u|` (closed form: `W_u` the offset's own u-partial,
    /// its speed); the joint expansion holds it within twice that. As the sum of the
    /// face's and the normal's expansions (the fork's), it came out `(R + r) / (R - r)`,
    /// about 2000 times, wider, and every order of the certificate's series again.
    #[test]
    fn the_offset_enclosure_keeps_the_focal_cancellation() {
        let (n, u, v, ku, kv) = cylinder_at();
        let pi = nurbs_partials::<Ivl>(&n, ku, kv, Ivl::from(u), Ivl::from(v), MAX_K);
        // the second face: the plane z = 0 by its parameters
        let mut p2 = Partials::<Ivl>::default();
        let iv = |x: f64, y: f64, z: f64| V3 {
            x: Ivl::from(x),
            y: Ivl::from(y),
            z: Ivl::from(z),
        };
        p2.set(1, 0, iv(1.0, 0.0, 0.0));
        p2.set(0, 1, iv(0.0, 1.0, 0.0));
        let pf = nurbs_partials::<f64>(&n, ku, kv, u, v, 2);
        // the side toward the axis
        let (s, su, sv) = (pf.at(0, 0), pf.at(1, 0), pf.at(0, 1));
        let outward = (su.y * sv.z - su.z * sv.y) * s.x + (su.z * sv.x - su.x * sv.z) * s.y;
        let side = if outward > 0.0 { -1.0 } else { 1.0 };
        let r = 9.99;
        let w_u = offset_at(&offset_taylor(&pf, r * side, 1).unwrap(), 1, 1, 0);
        let zero = Ser::constant(Ivl::from(0.0));
        let sys = GnSystem {
            p: [&pi, &p2],
            leader: VSer {
                a: [zero.clone(), zero.clone(), zero.clone()],
            },
            tangent: VSer {
                a: [zero.clone(), zero.clone(), zero],
            },
            r: [r, 1.0],
            side: [side, 1.0],
            hold: true,
            law: None,
            equal_setback: None,
        };
        let half = 1e-6;
        let mut xu = Ser::constant(Ivl::from(u));
        xu.set(1, 0, Ivl::new(1.0 - half, 1.0 + half));
        let x = [
            xu,
            Ser::constant(Ivl::from(v)),
            Ser::constant(Ivl::from(0.0)),
            Ser::constant(Ivl::from(0.0)),
        ];
        let (_, _, vv, _) = gn_evaluate_vn(&sys, &x, Trunc::new(1, 0).unwrap()).unwrap();
        for (c, wc) in vv.a.iter().zip([w_u.x, w_u.y, w_u.z]) {
            let got = c.at(1, 0);
            let width = got.hi - got.lo;
            let closed = 2.0 * half * wc.abs();
            assert!(
                width <= 2.0 * closed + 1e-12,
                "width {width:e} against the closed form's {closed:e}"
            );
        }
    }
}
