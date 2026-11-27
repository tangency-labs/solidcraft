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

use super::series::{inv_sqrt_scalar, GnScalar, InvSqrtSeries, Ser, Trunc, VSer, MAX_K, MAX_S, V3};
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
#[derive(Clone, Debug)]
pub struct Partials<T> {
    stride: usize,
    p: Vec<V3<T>>,
}

impl<T: GnScalar> PartialEq for Partials<T> {
    fn eq(&self, other: &Self) -> bool {
        if self.stride == other.stride {
            return self.p == other.p;
        }
        let n = self.stride.max(other.stride);
        for a in 0..n {
            for b in 0..n {
                if self.at(a, b) != other.at(a, b) {
                    return false;
                }
            }
        }
        true
    }
}

impl<T: GnScalar> Default for Partials<T> {
    fn default() -> Self {
        Self::with_order(MAX_K)
    }
}

impl<T: GnScalar> Partials<T> {
    /// A zero-initialized partials table sized for orders up to `lk` (clamped to `MAX_K`).
    #[must_use]
    pub(crate) fn with_order(lk: usize) -> Self {
        let stride = lk.min(MAX_K) + 1;
        let z = V3 {
            x: T::of(0.0),
            y: T::of(0.0),
            z: T::of(0.0),
        };
        Self {
            stride,
            p: vec![z; stride * stride],
        }
    }

    fn grow_to(&mut self, new_stride: usize) {
        let z = V3 {
            x: T::of(0.0),
            y: T::of(0.0),
            z: T::of(0.0),
        };
        let mut next = vec![z; new_stride * new_stride];
        for a in 0..self.stride {
            for b in 0..self.stride {
                if let (Some(&v), Some(slot)) = (
                    self.p.get(a * self.stride + b),
                    next.get_mut(a * new_stride + b),
                ) {
                    *slot = v;
                }
            }
        }
        self.stride = new_stride;
        self.p = next;
    }

    /// `P[a][b]` (0 past `MAX_K`).
    #[must_use]
    pub fn at(&self, a: usize, b: usize) -> V3<T> {
        let z = V3 {
            x: T::of(0.0),
            y: T::of(0.0),
            z: T::of(0.0),
        };
        if a >= self.stride || b >= self.stride {
            return z;
        }
        self.p.get(a * self.stride + b).copied().unwrap_or(z)
    }

    /// Sets `P[a][b]` (ignored past `MAX_K`).
    pub fn set(&mut self, a: usize, b: usize, v: V3<T>) {
        if a > MAX_K || b > MAX_K {
            return;
        }
        if a >= self.stride || b >= self.stride {
            self.grow_to(MAX_K + 1);
        }
        if let Some(x) = self.p.get_mut(a * self.stride + b) {
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

/// Flat matrix version of [`ders_basis`], returning `ders.at(r, j)` for `0 <= r <= d`,
/// `0 <= j <= p` without allocating a `Vec<Vec<T>>`.
#[must_use]
pub(crate) fn ders_basis_flat<T: GnScalar>(
    u: &[f64],
    k: usize,
    p: usize,
    x: T,
    d: usize,
) -> Mat<T> {
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
    ders
}

/// `sm_DersBasis`: the non-zero basis functions `N_{k-p..k}` and their derivatives up to
/// `d` at `x` in span `k`, `ders[d][j]` (the NURBS book's A2.3, its knot differences taken
/// from the knots directly, in `T`: for an interval `T` every result holds the exact one).
#[must_use]
pub fn ders_basis<T: GnScalar>(u: &[f64], k: usize, p: usize, x: T, d: usize) -> Vec<Vec<T>> {
    let ders = ders_basis_flat(u, k, p, x, d);
    (0..=d as isize)
        .map(|i| (0..=p as isize).map(|j| ders.at(i, j)).collect())
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
    let mut out = Partials::with_order(lk);
    let nu = ders_basis_flat(&n.u, ku, n.p, u, lk);
    let nv = ders_basis_flat(&n.v, kv, n.q, v, lk);
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
                    let c = nu.at(a as isize, i as isize) * nv.at(b as isize, j as isize);
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
    let mut out = Partials::with_order(lk);
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
    let g0 = inv_sqrt_scalar(h0)?;
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
    let zero_s = T::of(0.0);
    let zero = V3 {
        x: zero_s,
        y: zero_s,
        z: zero_s,
    };
    let lm = tr.s() + tr.e();
    let top = lm.min(tr.degree());
    let mut v_out = [VSer::constant(zero), VSer::constant(zero)];
    let mut f_out = [VSer::constant(zero), VSer::constant(zero)];
    let mut n_out = [VSer::constant(zero), VSer::constant(zero)];
    let mut du = Ser::constant(zero_s);
    let mut dv = Ser::constant(zero_s);
    let mut mono = Ser::constant(zero_s);
    let mut su = VSer::constant(zero);
    let mut sv = VSer::constant(zero);
    let mut nrm = VSer::constant(zero);
    let mut nn = Ser::constant(zero_s);
    let mut wv = VSer::constant(zero);
    let mut d = Ser::constant(zero_s);
    let mut w = Ser::constant(zero_s);
    let mut pu: Vec<Ser<T>> = Vec::new();
    let mut pv: Vec<Ser<T>> = Vec::new();
    pu.push(Ser::constant(T::of(1.0)));
    pv.push(Ser::constant(T::of(1.0)));
    for _ in 1..=top {
        pu.push(Ser::constant(zero_s));
        pv.push(Ser::constant(zero_s));
    }
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
        du.copy_trunc_from(xu, tr);
        dv.copy_trunc_from(xv, tr);
        du.set(0, 0, zero_s);
        dv.set(0, 0, zero_s);
        // the offset point's own expansion (enclosures near a focal distance, #16652,
        // #19306): its constant offset d0
        let d0 = offset_d0(sys, rf, sidef);
        let wt = if sys.equal_setback.is_none() && offset_near_focal(pf, d0) {
            Some(offset_taylor(pf, d0, top)?)
        } else {
            None
        };
        for a in 1..=top {
            let (Some((lu_slice, pu_slice)), Some((lv_slice, pv_slice))) =
                (pu.split_at_mut_checked(a), pv.split_at_mut_checked(a))
            else {
                return Err(SolidError::blend(
                    OP,
                    BlendFailure::Defect {
                        why: "the face's powers lack an order after preallocating to top",
                    },
                ));
            };
            let (Some(lu), Some(pua), Some(lv), Some(pva)) = (
                lu_slice.last(),
                pu_slice.first_mut(),
                lv_slice.last(),
                pv_slice.first_mut(),
            ) else {
                return Err(SolidError::blend(
                    OP,
                    BlendFailure::Defect {
                        why: "the face's powers lack the previous order: they start with the \
                              0th and each order is preallocated up to top",
                    },
                ));
            };
            lu.mul_valued_into(&du, a - 1, 1, tr, pua);
            lv.mul_valued_into(&dv, a - 1, 1, tr, pva);
        }
        let Some(s) = f_out.get_mut(f) else {
            return Err(SolidError::blend(
                OP,
                BlendFailure::Defect {
                    why: "the output feet array lacks face f",
                },
            ));
        };
        if f > 0 {
            su.zero_computed(tr);
            sv.zero_computed(tr);
            if wt.is_some() {
                wv.zero_computed(tr);
            }
        }
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
                pua.mul_valued_into(pvb, a, b, tr, &mut mono);
                // a! b! is exact, 1 / (a! b!) is rounded in T (#2559)
                let sf = T::of(1.0) / T::of(fa * fb);
                let cs = [pf.at(a, b), pf.at(a + 1, b), pf.at(a, b + 1)];
                let outs: [&mut VSer<T>; 3] = [s, &mut su, &mut sv];
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
        su.cross_into(&sv, tr, &mut nrm);
        nrm.dot_into(&nrm, tr, &mut nn);
        let inv_len = nn.inv_sqrt(tr)?;
        let Some(no) = n_out.get_mut(f) else {
            return Err(SolidError::blend(
                OP,
                BlendFailure::Defect {
                    why: "the output normals array lacks face f",
                },
            ));
        };
        nrm.times_into(&inv_len, tr, no);
        if sys.equal_setback.is_some() {
            continue;
        }
        // the offset towards the ball: tau r_i s_i, tau = 1 + eps (r_i a series in sigma
        // for a law)
        for i in 0..=tr.s() {
            d.set(i, 0, zero_s);
            if tr.e() >= 1 {
                d.set(i, 1, zero_s);
            }
        }
        d.set(0, 0, T::of(rf * sidef));
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
        let Some(vo) = v_out.get_mut(f) else {
            return Err(SolidError::blend(
                OP,
                BlendFailure::Defect {
                    why: "the output offset array lacks face f",
                },
            ));
        };
        if wt.is_some() {
            if offset_varies(sys, tr) {
                d.set(0, 0, zero_s);
                inv_len.mul_into(&d, tr, &mut w);
                wv.add_times_into(&nrm, &w, tr, vo);
            } else {
                vo.merge_computed(&wv, tr);
            }
        } else {
            inv_len.mul_into(&d, tr, &mut w);
            s.add_times_into(&nrm, &w, tr, vo);
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
            offset.scale_into(T::of(side), tr, &mut w);
            foot.add_times_into(normal, &w, tr, centre);
        }
    }
    let [v0, v1] = v_out;
    let diff = v0.sub(&v1, tr);
    let [g0, g1, g2] = diff.a;
    v0.sub_into(&sys.leader, tr, &mut su);
    let g3 = su.dot(&sys.tangent, tr);
    Ok(([g0, g1, g2, g3], f_out, v0, n_out))
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

/// Which unit-normal coefficients [`GnIncremental`] should bring up to date.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NeedNormal {
    None,
    SigmaOnly,
    Full,
}

/// [`gn_evaluate_vn`] evaluated incrementally, for a caller that evaluates the same system
/// again and again as it fills in the feet's series (the certificate's Taylor models and
/// interval enclosures solve them order by order): every series [`gn_evaluate_vn`] takes
/// is kept, and an evaluation computes only the coefficients its truncation computes that
/// no earlier one has computed from the same coefficients of x. Each coefficient depends
/// only on x's coefficients at the same or lower powers of both sigma and eps (see
/// [`Trunc`]), so a kept one stays valid until x changes in that quadrant, and the
/// evaluation's operations, their order and so every value are [`gn_evaluate_vn`]'s, bit
/// for bit: the two must change together (a test compares them).
#[derive(Clone, Debug)]
pub struct GnIncremental<'s, 'a, T: GnScalar> {
    sys: &'s GnSystem<'a, T>,
    /// The orders of the kept series (`Trunc::s`, `Trunc::e`), none before the first.
    orders: Option<(usize, usize)>,
    /// The x the kept series were computed from.
    x: [Ser<T>; 4],
    /// The kept coefficients still valid ([`Trunc::bit`]s, the same for every series
    /// except `faces[f].n`).
    valid: u64,
    /// The kept coefficients of `faces[f].n` still valid.
    n_valid: u64,
    faces: [GnFaceSeries<T>; 2],
    g: [Ser<T>; 4],
    /// `V - leader` (its dot product with the tangent reads all its lower coefficients).
    t: VSer<T>,
    /// Reusable scratch series for incremental updates (zero allocations per call).
    du: Ser<T>,
    dv: Ser<T>,
    mono: Ser<T>,
    d: Ser<T>,
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
            n_valid: 0,
            faces: [GnFaceSeries::new(), GnFaceSeries::new()],
            g: core::array::from_fn(|_| z.clone()),
            t: VSer {
                a: core::array::from_fn(|_| z.clone()),
            },
            du: z.clone(),
            dv: z.clone(),
            mono: z.clone(),
            d: z,
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
        if let Err(e) = self.evaluate_core(x, tr, NeedNormal::Full) {
            self.valid = 0;
            self.n_valid = 0;
            self.orders = None;
            return Err(e);
        }
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

    /// Updates `G` up to `tr` in place and returns a borrow of `G` with zero output
    /// allocations (skipping unit normals, which order-by-order solves do not read).
    /// Every coefficient computed by `tr` matches [`gn_evaluate_vn`] bit for bit.
    ///
    /// # Errors
    ///
    /// As [`gn_evaluate_vn`].
    pub fn evaluate_g(&mut self, x: &[Ser<T>; 4], tr: Trunc) -> Result<&[Ser<T>; 4]> {
        if self.sys.equal_setback.is_some() {
            self.computed = self
                .computed
                .saturating_add(u64::from(tr.computed_mask().count_ones()));
            let (g, _, _, _) = gn_evaluate_vn(self.sys, x, tr)?;
            self.g = g;
            return Ok(&self.g);
        }
        if let Err(e) = self.evaluate_core(x, tr, NeedNormal::None) {
            self.valid = 0;
            self.n_valid = 0;
            self.orders = None;
            return Err(e);
        }
        Ok(&self.g)
    }

    /// Completes the incremental evaluation at `tr` (computing unit normals only where
    /// `need_normal_sigma` is true, and then only to `Trunc(tr.s(), 0)` for
    /// [`gn_top_jets`]). Follow with [`Self::into_fv`] to move the series out without
    /// cloning.
    ///
    /// # Errors
    ///
    /// As [`gn_evaluate_vn`].
    pub fn evaluate_final(
        &mut self,
        x: &[Ser<T>; 4],
        tr: Trunc,
        need_normal_sigma: bool,
    ) -> Result<()> {
        if self.sys.equal_setback.is_some() {
            self.computed = self
                .computed
                .saturating_add(u64::from(tr.computed_mask().count_ones()));
            let (g, [fs0, fs1], v, [fn0, fn1]) = gn_evaluate_vn(self.sys, x, tr)?;
            self.g = g;
            let [f0, f1] = &mut self.faces;
            f0.s = fs0;
            f1.s = fs1;
            f0.v = v;
            f0.n = fn0;
            f1.n = fn1;
            return Ok(());
        }
        let need_n = if need_normal_sigma {
            NeedNormal::SigmaOnly
        } else {
            NeedNormal::None
        };
        if let Err(e) = self.evaluate_core(x, tr, need_n) {
            self.valid = 0;
            self.n_valid = 0;
            self.orders = None;
            return Err(e);
        }
        Ok(())
    }

    /// Consumes the evaluator after [`Self::evaluate_final`], masking and moving out
    /// `(f, v, n)` with zero allocations.
    #[must_use]
    #[allow(clippy::type_complexity)]
    pub fn into_fv(
        self,
        tr: Trunc,
        need_normal_sigma: bool,
    ) -> ([VSer<T>; 2], VSer<T>, Option<[VSer<T>; 2]>) {
        let [mut f0, mut f1] = self.faces;
        f0.s.mask_in_place(tr);
        f1.s.mask_in_place(tr);
        f0.v.mask_in_place(tr);
        let n = if need_normal_sigma {
            let sigma = Trunc::new(tr.s(), 0).unwrap_or(tr);
            f0.n.mask_in_place(sigma);
            f1.n.mask_in_place(sigma);
            Some([f0.n, f1.n])
        } else {
            None
        };
        ([f0.s, f1.s], f0.v, n)
    }

    /// Completes the incremental evaluation at `tr` with full unit normals and moves
    /// `(f, v, n)` out without cloning.
    #[allow(clippy::type_complexity)]
    pub(crate) fn finish_vn(
        mut self,
        x: &[Ser<T>; 4],
        tr: Trunc,
    ) -> Result<([VSer<T>; 2], VSer<T>, [VSer<T>; 2])> {
        if self.sys.equal_setback.is_some() {
            self.computed = self
                .computed
                .saturating_add(u64::from(tr.computed_mask().count_ones()));
            let (_, f, v, n) = gn_evaluate_vn(self.sys, x, tr)?;
            return Ok((f, v, n));
        }
        self.evaluate_core(x, tr, NeedNormal::Full)?;
        let [mut f0, mut f1] = self.faces;
        f0.s.mask_in_place(tr);
        f1.s.mask_in_place(tr);
        f0.v.mask_in_place(tr);
        f0.n.mask_in_place(tr);
        f1.n.mask_in_place(tr);
        Ok(([f0.s, f1.s], f0.v, [f0.n, f1.n]))
    }

    fn evaluate_core(&mut self, x: &[Ser<T>; 4], tr: Trunc, need_n: NeedNormal) -> Result<()> {
        if self.orders != Some((tr.s(), tr.e())) {
            let computed = self.computed;
            *self = Self::new(self.sys);
            self.computed = computed;
            self.orders = Some((tr.s(), tr.e()));
        }
        // a coefficient of x that changed invalidates every kept one at or above both its
        // powers (within this evaluator's orders)
        let max_s = tr.s();
        let max_e = tr.e();
        for (xn, xo) in x.iter().zip(&mut self.x) {
            for i in 0..=max_s {
                for k in 0..=max_e {
                    if xn.at(i, k) != xo.at(i, k) {
                        for i2 in i..=max_s {
                            for k2 in k..=max_e {
                                let mask = !Trunc::bit(i2, k2);
                                self.valid &= mask;
                                self.n_valid &= mask;
                            }
                        }
                    }
                }
            }
            xo.copy_trunc_from(xn, tr);
        }
        let all = tr.computed_mask();
        let btr = tr.skipping(self.valid);
        let btr_mask = btr.computed_mask();
        self.computed = self
            .computed
            .saturating_add(u64::from(btr_mask.count_ones()));
        if btr_mask != 0 {
            let lm = tr.s() + tr.e();
            let top = lm.min(tr.degree());
            let zero_s = T::of(0.0);
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
                self.du.copy_trunc_from(xu, tr);
                self.dv.copy_trunc_from(xv, tr);
                self.du.set(0, 0, zero_s);
                self.dv.set(0, 0, zero_s);
                while fs.pu.len() <= top {
                    fs.pu.push(Ser::constant(zero_s));
                    fs.pv.push(Ser::constant(zero_s));
                }
                for a in 1..=top {
                    let (Some((lu_slice, pu_slice)), Some((lv_slice, pv_slice))) =
                        (fs.pu.split_at_mut_checked(a), fs.pv.split_at_mut_checked(a))
                    else {
                        return Err(SolidError::blend(
                            OP,
                            BlendFailure::Defect {
                                why: "the face's kept powers lack an order after resizing to top",
                            },
                        ));
                    };
                    let (Some(lu), Some(pua), Some(lv), Some(pva)) = (
                        lu_slice.last(),
                        pu_slice.first_mut(),
                        lv_slice.last(),
                        pv_slice.first_mut(),
                    ) else {
                        return Err(SolidError::blend(
                            OP,
                            BlendFailure::Defect {
                                why: "the face's kept powers lack an order: they start with the \
                                      0th and each order is resized up to top",
                            },
                        ));
                    };
                    lu.mul_valued_into(&self.du, a - 1, 1, btr, pua);
                    lv.mul_valued_into(&self.dv, a - 1, 1, btr, pva);
                }
                fs.s.zero_computed(btr);
                fs.su.zero_computed(btr);
                fs.sv.zero_computed(btr);
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
                if fs.wt.is_some() {
                    fs.wv.zero_computed(btr);
                }
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
                        pua.mul_valued_into(pvb, a, b, btr, &mut self.mono);
                        let sf = T::of(1.0) / T::of(fa * fb);
                        let cs = [pf.at(a, b), pf.at(a + 1, b), pf.at(a, b + 1)];
                        let outs: [&mut VSer<T>; 3] = [&mut fs.s, &mut fs.su, &mut fs.sv];
                        for (c, out) in cs.iter().zip(outs) {
                            let cv = [c.x, c.y, c.z];
                            for (o, &cj) in out.a.iter_mut().zip(&cv) {
                                o.add_scaled_from(&self.mono, cj * sf, a + b, btr);
                            }
                        }
                        if let Some(wt) = &fs.wt {
                            let c = offset_at(wt, lm, a, b);
                            for (o, &cj) in fs.wv.a.iter_mut().zip(&[c.x, c.y, c.z]) {
                                o.add_scaled_from(&self.mono, cj, a + b, btr);
                            }
                        }
                    }
                }
                fs.su.cross_into(&fs.sv, btr, &mut fs.nrm);
                fs.nrm.dot_into(&fs.nrm, btr, &mut fs.nn);
                let inv_len = fs.inv.update(&fs.nn, btr)?;
                for i in 0..=tr.s() {
                    self.d.set(i, 0, zero_s);
                    if tr.e() >= 1 {
                        self.d.set(i, 1, zero_s);
                    }
                }
                self.d.set(0, 0, T::of(rf * sidef));
                if let Some(law) = sys.law {
                    for i in 0..=tr.s() {
                        self.d.set(i, 0, law.at(i, 0) * T::of(sidef));
                    }
                }
                if !sys.hold && tr.e() >= 1 {
                    for i in 0..=tr.s() {
                        self.d.set(i, 1, self.d.at(i, 0));
                    }
                }
                // (kept, as every series a later product reads: a product's coefficient reads
                // its operands' lower ones, which an incremental call does not compute again)
                if fs.wt.is_some() {
                    if offset_varies(sys, tr) {
                        self.d.set(0, 0, zero_s);
                        inv_len.mul_into(&self.d, btr, &mut fs.w);
                        fs.wv.add_times_into(&fs.nrm, &fs.w, btr, &mut fs.v);
                    } else {
                        fs.v.merge_computed(&fs.wv, btr);
                    }
                } else {
                    inv_len.mul_into(&self.d, btr, &mut fs.w);
                    fs.s.add_times_into(&fs.nrm, &fs.w, btr, &mut fs.v);
                }
            }
            let [f0, f1] = &self.faces;
            for ((g, v0), v1) in self.g.iter_mut().zip(&f0.v.a).zip(&f1.v.a) {
                v0.sub_into(v1, btr, g);
            }
            f0.v.sub_into(&sys.leader, btr, &mut self.t);
            if let Some(g3) = self.g.get_mut(3) {
                self.t.dot_into(&sys.tangent, btr, g3);
            }
            self.valid |= all;
        }
        match need_n {
            NeedNormal::None => {}
            NeedNormal::SigmaOnly => {
                let sigma = Trunc::new(tr.s(), 0)?.computing(tr.s().min(tr.degree()), 0);
                let ntr = sigma.skipping(self.n_valid);
                if ntr.computed_mask() != 0 {
                    for fs in &mut self.faces {
                        fs.nrm.times_into(fs.inv.out(), ntr, &mut fs.n);
                    }
                }
                self.n_valid |= sigma.computed_mask();
            }
            NeedNormal::Full => {
                let ntr = tr.skipping(self.n_valid);
                if ntr.computed_mask() != 0 {
                    for fs in &mut self.faces {
                        fs.nrm.times_into(fs.inv.out(), ntr, &mut fs.n);
                    }
                }
                self.n_valid |= all;
            }
        }
        Ok(())
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
    if top < 1 || top > tr.e() {
        return;
    }
    let sigma = Trunc::new(tr.s(), 0).unwrap_or(tr);
    let zero_s = T::of(0.0);
    let zero = V3 {
        x: zero_s,
        y: zero_s,
        z: zero_s,
    };
    let mut sn = VSer::constant(zero);
    let mut sc = VSer::constant(zero);
    let mut dot = Ser::constant(zero_s);
    let mut sj = VSer::constant(zero);
    for (ff, nf) in f.iter_mut().zip(n) {
        for (dst, src) in sn.a.iter_mut().zip(&nf.a) {
            for i in 0..=tr.s() {
                dst.set(i, 0, src.at(i, 0));
            }
        }
        for kk in top..=tr.e() {
            for (cc, fc) in sc.a.iter_mut().zip(&ff.a) {
                for i in 0..=tr.s() {
                    cc.set(i, 0, fc.at(i, kk));
                }
            }
            sn.dot_into(&sc, sigma, &mut dot);
            sn.times_into(&dot, sigma, &mut sj);
            for (fc, jc) in ff.a.iter_mut().zip(&sj.a) {
                for i in 0..=tr.s() {
                    fc.set(i, kk, jc.at(i, 0));
                }
            }
        }
    }
}

/// First-order dual number `c0 + c1 eps` (`Trunc(0, 1)` on the stack, zero heap
/// allocations), reproducing every [`Ser<T>`] operation's rounding order bit for bit.
#[derive(Clone, Copy)]
struct Dual<T> {
    c0: T,
    c1: T,
}

impl<T: GnScalar> Dual<T> {
    #[inline]
    fn new(c0: T, c1: T) -> Self {
        Self { c0, c1 }
    }

    #[inline]
    fn zero() -> Self {
        let z = T::of(0.0);
        Self { c0: z, c1: z }
    }

    #[inline]
    fn add(self, b: Self) -> Self {
        Self {
            c0: self.c0 + b.c0,
            c1: self.c1 + b.c1,
        }
    }

    #[inline]
    fn sub(self, b: Self) -> Self {
        Self {
            c0: self.c0 - b.c0,
            c1: self.c1 - b.c1,
        }
    }

    #[inline]
    fn scale(self, f: T) -> Self {
        Self {
            c0: self.c0 * f,
            c1: self.c1 * f,
        }
    }

    #[inline]
    fn mul(self, b: Self) -> Self {
        let z = T::of(0.0);
        Self {
            c0: z + self.c0 * b.c0,
            c1: (z + self.c0 * b.c1) + self.c1 * b.c0,
        }
    }

    #[inline]
    fn inv_sqrt(self) -> Result<Self> {
        let x0 = self.c0;
        if !x0.is_positive() {
            return Err(SolidError::blend(
                "blend::fillet_section::Ser::inv_sqrt",
                BlendFailure::SectionDegenerate(SectionDegeneracy::VectorVanishes),
            ));
        }
        let z = T::of(0.0);
        let one = T::of(1.0);
        let z1 = self.c1 * (one / x0);
        let pow1 = z + one * z1;
        let sum1 = z + pow1 * T::of(-0.5);
        let inv_s = one / x0.sqrt();
        Ok(Self {
            c0: one * inv_s,
            c1: sum1 * inv_s,
        })
    }
}

/// 3D vector of [`Dual<T>`].
#[derive(Clone, Copy)]
struct V3Dual<T>([Dual<T>; 3]);

impl<T: GnScalar> V3Dual<T> {
    #[inline]
    fn zero() -> Self {
        let z = Dual::zero();
        Self([z, z, z])
    }

    #[inline]
    fn add(self, b: Self) -> Self {
        let [a0, a1, a2] = self.0;
        let [b0, b1, b2] = b.0;
        Self([a0.add(b0), a1.add(b1), a2.add(b2)])
    }

    #[inline]
    fn sub(self, b: Self) -> Self {
        let [a0, a1, a2] = self.0;
        let [b0, b1, b2] = b.0;
        Self([a0.sub(b0), a1.sub(b1), a2.sub(b2)])
    }

    #[inline]
    fn times(self, s: Dual<T>) -> Self {
        let [a0, a1, a2] = self.0;
        Self([a0.mul(s), a1.mul(s), a2.mul(s)])
    }

    #[inline]
    fn dot(self, b: Self) -> Dual<T> {
        let [a0, a1, a2] = self.0;
        let [b0, b1, b2] = b.0;
        a0.mul(b0).add(a1.mul(b1)).add(a2.mul(b2))
    }

    #[inline]
    fn cross(self, b: Self) -> Self {
        let [a0, a1, a2] = self.0;
        let [b0, b1, b2] = b.0;
        Self([
            a1.mul(b2).sub(a2.mul(b1)),
            a2.mul(b0).sub(a0.mul(b2)),
            a0.mul(b1).sub(a1.mul(b0)),
        ])
    }
}

/// Constant term `W[0][0]` of [`offset_taylor`] without allocating tables.
fn offset_taylor_deg0<T: GnScalar>(pf: &Partials<T>, d0: T) -> Result<V3<T>> {
    let zero = T::of(0.0);
    let one = T::of(1.0);
    #[allow(clippy::eq_op)]
    let sf = one / one;
    let scale = |p: V3<T>, f: T| V3 {
        x: p.x * f,
        y: p.y * f,
        z: p.z * f,
    };
    let t00 = scale(pf.at(0, 0), sf);
    let su = scale(scale(pf.at(1, 0), sf), one);
    let sv = scale(scale(pf.at(0, 1), sf), one);
    let n = V3 {
        x: zero + (su.y * sv.z - su.z * sv.y),
        y: zero + (su.z * sv.x - su.x * sv.z),
        z: zero + (su.x * sv.y - su.y * sv.x),
    };
    let h0 = zero + (n.x * n.x + n.y * n.y + n.z * n.z);
    let g0 = inv_sqrt_scalar(h0)?;
    let c = V3 {
        x: zero + n.x * g0,
        y: zero + n.y * g0,
        z: zero + n.z * g0,
    };
    Ok(V3 {
        x: t00.x + d0 * c.x,
        y: t00.y + d0 * c.y,
        z: t00.z + d0 * c.z,
    })
}

/// Evaluates `G` at `Trunc(0, 0)` directly on the stack (zero heap allocations), matching
/// `gn_evaluate(sys, x, Trunc(0, 0))` bit for bit.
#[allow(clippy::too_many_arguments)]
pub(crate) fn gn_evaluate_point_ref<T: GnScalar>(
    p: [&Partials<T>; 2],
    leader: &VSer<T>,
    tangent: &VSer<T>,
    r: [f64; 2],
    side: [f64; 2],
    law: Option<&Ser<T>>,
    equal_setback: Option<f64>,
) -> Result<[T; 4]> {
    let zero = T::of(0.0);
    let one = T::of(1.0);
    let vz = V3 {
        x: zero,
        y: zero,
        z: zero,
    };
    #[allow(clippy::eq_op)]
    let sf = one / one;
    let mono0 = zero + one * one;
    let mut v_out = [vz; 2];
    let mut f_out = [vz; 2];
    let mut n_out = [vz; 2];
    for (f, ((pf, &rf), sidef)) in p.into_iter().zip(r.iter()).zip(side).enumerate() {
        let d0 = match law {
            Some(l) => l.at(0, 0) * T::of(sidef),
            None => T::of(rf * sidef),
        };
        let near = equal_setback.is_none() && offset_near_focal(pf, d0);
        let scaled = |c: V3<T>| V3 {
            x: zero + mono0 * (c.x * sf),
            y: zero + mono0 * (c.y * sf),
            z: zero + mono0 * (c.z * sf),
        };
        let s = scaled(pf.at(0, 0));
        let su = scaled(pf.at(1, 0));
        let sv = scaled(pf.at(0, 1));
        let nrm = V3 {
            x: (zero + su.y * sv.z) - (zero + su.z * sv.y),
            y: (zero + su.z * sv.x) - (zero + su.x * sv.z),
            z: (zero + su.x * sv.y) - (zero + su.y * sv.x),
        };
        let nn = ((zero + nrm.x * nrm.x) + (zero + nrm.y * nrm.y)) + (zero + nrm.z * nrm.z);
        let inv_len = inv_sqrt_scalar(nn)?;
        if equal_setback.is_some() {
            if let (Some(fo), Some(no)) = (f_out.get_mut(f), n_out.get_mut(f)) {
                *fo = s;
                *no = V3 {
                    x: zero + nrm.x * inv_len,
                    y: zero + nrm.y * inv_len,
                    z: zero + nrm.z * inv_len,
                };
            }
            continue;
        }
        let vf = if near {
            let w00 = offset_taylor_deg0(pf, d0)?;
            let wv = V3 {
                x: zero + mono0 * w00.x,
                y: zero + mono0 * w00.y,
                z: zero + mono0 * w00.z,
            };
            if law.is_some() {
                let dw = zero + inv_len * zero;
                V3 {
                    x: wv.x + (zero + nrm.x * dw),
                    y: wv.y + (zero + nrm.y * dw),
                    z: wv.z + (zero + nrm.z * dw),
                }
            } else {
                wv
            }
        } else {
            let dw = zero + inv_len * d0;
            V3 {
                x: s.x + (zero + nrm.x * dw),
                y: s.y + (zero + nrm.y * dw),
                z: s.z + (zero + nrm.z * dw),
            }
        };
        if let Some(vo) = v_out.get_mut(f) {
            *vo = vf;
        }
    }
    if let Some(distance) = equal_setback {
        if !(distance.is_finite() && distance > 0.0) {
            return Err(SolidError::invalid_input(
                OP,
                "the chamfer's setback is finite and positive",
            ));
        }
        let [n0, n1] = n_out;
        let [s0, s1] = side;
        let q =
            (((zero + n0.x * n1.x) + (zero + n0.y * n1.y)) + (zero + n0.z * n1.z)) * T::of(s0 * s1);
        let one_plus = one + q;
        let one_minus = one - q;
        if !(one_plus.is_positive() && one_minus.is_positive()) {
            return Err(SolidError::blend(
                OP,
                BlendFailure::SectionNotProved(SectionProof::RadiusPositive),
            ));
        }
        let inv_sq = inv_sqrt_scalar(zero + one_plus * one_minus)?;
        let radius = (zero + one_plus * inv_sq) * T::of(distance);
        let offset = zero + radius * one;
        for (((centre, foot), normal), sidef) in v_out.iter_mut().zip(f_out).zip(n_out).zip(side) {
            let os = offset * T::of(sidef);
            *centre = V3 {
                x: foot.x + (zero + normal.x * os),
                y: foot.y + (zero + normal.y * os),
                z: foot.z + (zero + normal.z * os),
            };
        }
    }
    let [v0, v1] = v_out;
    let [l0, l1, l2] = &leader.a;
    let [t0, t1, t2] = &tangent.a;
    let tl = V3 {
        x: v0.x - l0.at(0, 0),
        y: v0.y - l1.at(0, 0),
        z: v0.z - l2.at(0, 0),
    };
    let g3 =
        ((zero + tl.x * t0.at(0, 0)) + (zero + tl.y * t1.at(0, 0))) + (zero + tl.z * t2.at(0, 0));
    Ok([v0.x - v1.x, v0.y - v1.y, v0.z - v1.z, g3])
}

/// Evaluates one face's foot, unit normal, and (when `equal_setback` is false) offset point
/// at `Trunc(0, 1)` with `hold = true` and `du = (0, du1)`, `dv = (0, dv1)`, matching
/// [`gn_evaluate_vn`] bit for bit.
#[allow(clippy::too_many_arguments)]
fn eval_face_dual<T: GnScalar>(
    pf: &Partials<T>,
    d0: T,
    has_law: bool,
    equal_setback: bool,
    du1: T,
    dv1: T,
    wt: Option<&[V3<T>]>,
) -> Result<(V3Dual<T>, V3Dual<T>, V3Dual<T>)> {
    let zero = T::of(0.0);
    let one = T::of(1.0);
    #[allow(clippy::eq_op)]
    let sf = one / one;
    let pu1 = zero + one * du1;
    let pv1 = zero + one * dv1;
    let mono00 = Dual::new(zero + one * one, (zero + one * zero) + zero * one);
    let mono01 = Dual::new(zero, zero + one * pv1);
    let mono10 = Dual::new(zero, zero + pu1 * one);
    let mut s = V3Dual::zero();
    let mut su = V3Dual::zero();
    let mut sv = V3Dual::zero();
    let mut wv = V3Dual::zero();
    let accum = |out: &mut V3Dual<T>, mono: Dual<T>, c: V3<T>, from: usize| {
        for (o, cj) in out.0.iter_mut().zip([c.x, c.y, c.z]) {
            if from == 0 {
                o.c0 = o.c0 + mono.c0 * cj;
            }
            o.c1 = o.c1 + mono.c1 * cj;
        }
    };
    let scale_v3 = |c: V3<T>| V3 {
        x: c.x * sf,
        y: c.y * sf,
        z: c.z * sf,
    };
    for (a, b, mono) in [(0, 0, mono00), (0, 1, mono01), (1, 0, mono10)] {
        let from = a + b;
        accum(&mut s, mono, scale_v3(pf.at(a, b)), from);
        accum(&mut su, mono, scale_v3(pf.at(a + 1, b)), from);
        accum(&mut sv, mono, scale_v3(pf.at(a, b + 1)), from);
        if let Some(wt) = wt {
            accum(&mut wv, mono, offset_at(wt, 1, a, b), from);
        }
    }
    let nrm = su.cross(sv);
    let inv_len = nrm.dot(nrm).inv_sqrt()?;
    if equal_setback {
        return Ok((s, nrm.times(inv_len), V3Dual::zero()));
    }
    let d = Dual::new(d0, zero);
    let vf = if wt.is_some() {
        if has_law {
            wv.add(nrm.times(inv_len.mul(Dual::zero())))
        } else {
            wv
        }
    } else {
        s.add(nrm.times(inv_len.mul(d)))
    };
    Ok((s, V3Dual::zero(), vf))
}

/// Equal-setback radius in [`Dual<T>`] (`Trunc(0, 1)`), matching [`equal_setback_radius`]
/// bit for bit.
fn equal_setback_radius_dual<T: GnScalar>(
    normals: [V3Dual<T>; 2],
    side: [f64; 2],
    distance: f64,
) -> Result<Dual<T>> {
    if !(distance.is_finite() && distance > 0.0) {
        return Err(SolidError::invalid_input(
            OP,
            "the chamfer's setback is finite and positive",
        ));
    }
    let [first, second] = normals;
    let [side_first, side_second] = side;
    let q = first.dot(second).scale(T::of(side_first * side_second));
    let one = Dual::new(T::of(1.0), T::of(0.0));
    let one_plus = one.add(q);
    let one_minus = one.sub(q);
    if !(one_plus.c0.is_positive() && one_minus.c0.is_positive()) {
        return Err(SolidError::blend(
            OP,
            BlendFailure::SectionNotProved(SectionProof::RadiusPositive),
        ));
    }
    let inv_sqrt = one_plus.mul(one_minus).inv_sqrt()?;
    Ok(one_plus.mul(inv_sqrt).scale(T::of(distance)))
}

/// Evaluates both the residual `G(x)` at `tau = 1` and the Jacobian `dG/dx` on the stack
/// without cloning `leader`/`tangent` or allocating series, matching [`gn_linearize`] bit
/// for bit.
#[allow(clippy::too_many_arguments)]
pub(crate) fn gn_linearize_ref<T: GnScalar>(
    p: [&Partials<T>; 2],
    leader: &VSer<T>,
    tangent: &VSer<T>,
    r: [f64; 2],
    side: [f64; 2],
    law: Option<&Ser<T>>,
    equal_setback: Option<f64>,
) -> Result<([T; 4], [[T; 4]; 4])> {
    let zero = T::of(0.0);
    let one = T::of(1.0);
    let [p0, p1] = p;
    let d0_of = |rf: f64, sidef: f64| match law {
        Some(l) => l.at(0, 0) * T::of(sidef),
        None => T::of(rf * sidef),
    };
    let d00 = d0_of(r[0], side[0]);
    let d01 = d0_of(r[1], side[1]);
    let has_law = law.is_some();
    let eq = equal_setback.is_some();
    let wt0 = if !eq && offset_near_focal(p0, d00) {
        Some(offset_taylor(p0, d00, 1)?)
    } else {
        None
    };
    let wt1 = if !eq && offset_near_focal(p1, d01) {
        Some(offset_taylor(p1, d01, 1)?)
    } else {
        None
    };
    let f0_const = eval_face_dual(p0, d00, has_law, eq, zero, zero, wt0.as_deref())?;
    let f1_const = eval_face_dual(p1, d01, has_law, eq, zero, zero, wt1.as_deref())?;
    let f0_u = eval_face_dual(p0, d00, has_law, eq, one, zero, wt0.as_deref())?;
    let f0_v = eval_face_dual(p0, d00, has_law, eq, zero, one, wt0.as_deref())?;
    let f1_u = eval_face_dual(p1, d01, has_law, eq, one, zero, wt1.as_deref())?;
    let f1_v = eval_face_dual(p1, d01, has_law, eq, zero, one, wt1.as_deref())?;
    let [l0, l1, l2] = &leader.a;
    let [t0, t1, t2] = &tangent.a;
    let leader_d = V3Dual([
        Dual::new(l0.at(0, 0), l0.at(0, 1)),
        Dual::new(l1.at(0, 0), l1.at(0, 1)),
        Dual::new(l2.at(0, 0), l2.at(0, 1)),
    ]);
    let tangent_d = V3Dual([
        Dual::new(t0.at(0, 0), t0.at(0, 1)),
        Dual::new(t1.at(0, 0), t1.at(0, 1)),
        Dual::new(t2.at(0, 0), t2.at(0, 1)),
    ]);
    let pairs = [
        (f0_u, f1_const),
        (f0_v, f1_const),
        (f0_const, f1_u),
        (f0_const, f1_v),
    ];
    let mut residual = [zero; 4];
    let mut jac = [[zero; 4]; 4];
    let tau_one = Dual::new(one, zero);
    for (j, ((s0, n0, mut v0), (s1, n1, mut v1))) in pairs.into_iter().enumerate() {
        if let Some(distance) = equal_setback {
            let radius = equal_setback_radius_dual([n0, n1], side, distance)?;
            let offset = radius.mul(tau_one);
            v0 = s0.add(n0.times(offset.scale(T::of(side[0]))));
            v1 = s1.add(n1.times(offset.scale(T::of(side[1]))));
        }
        let diff = v0.sub(v1);
        let g3 = v0.sub(leader_d).dot(tangent_d);
        let [d0, d1, d2] = diff.0;
        let g = [d0, d1, d2, g3];
        if j == 0 {
            residual = g.map(|gi| gi.c0);
        }
        for (row, gi) in jac.iter_mut().zip(g) {
            if let Some(slot) = row.get_mut(j) {
                *slot = gi.c1;
            }
        }
    }
    Ok((residual, jac))
}

/// [`gn_jacobian`] borrowing `leader` and `tangent` with zero series allocations.
#[allow(clippy::too_many_arguments)]
pub(crate) fn gn_jacobian_ref<T: GnScalar>(
    p: [&Partials<T>; 2],
    leader: &VSer<T>,
    tangent: &VSer<T>,
    r: [f64; 2],
    side: [f64; 2],
    law: Option<&Ser<T>>,
    equal_setback: Option<f64>,
) -> Result<[[T; 4]; 4]> {
    Ok(gn_linearize_ref(p, leader, tangent, r, side, law, equal_setback)?.1)
}

/// `sm_GnJacobian`: the Jacobian `dG/dx` at x's constant terms and tau = 1: G along
/// `x + e_j eps`, first order. An equal setback's offset still follows the feet.
///
/// # Errors
///
/// Those of [`gn_evaluate`].
pub fn gn_jacobian<T: GnScalar>(sys: &GnSystem<'_, T>, x: &[Ser<T>; 4]) -> Result<[[T; 4]; 4]> {
    Ok(gn_linearize(sys, x)?.1)
}

/// Each seeded column has the same constant term, G(x) at tau = 1.
fn gn_linearize<T: GnScalar>(
    sys: &GnSystem<'_, T>,
    _x: &[Ser<T>; 4],
) -> Result<([T; 4], [[T; 4]; 4])> {
    gn_linearize_ref(
        sys.p,
        &sys.leader,
        &sys.tangent,
        sys.r,
        sys.side,
        sys.law,
        sys.equal_setback,
    )
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
    let mut converged = false;
    let mut it = 0;
    // the last iteration's residual and step, for a station that does not converge
    let mut last: Option<(f64, f64)> = None;
    while it < 50 && !converged {
        *steps += 1;
        let pp = partials_at(&x, 2)?;
        let [p0, p1] = &pp;
        let (g, jac) = gn_linearize_ref([p0, p1], &l, &t, r, side, law, equal_setback)?;
        let mut b = [0.0_f64; 4];
        let mut res = 0.0_f64;
        for (bi, gi) in b.iter_mut().zip(&g) {
            *bi = -gi;
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
    let jac = gn_jacobian_ref(
        [p0, p1],
        &sys.leader,
        &sys.tangent,
        r,
        side,
        law,
        equal_setback,
    )?;
    // (the coefficients of one total degree do not enter each other's: one evaluation
    // solves them all; G is computed only at that degree and below, and at the powers of
    // eps solved for, bit for bit the whole truncation's there (`Trunc::computing`), and
    // not at all at an order with none to solve)
    let eps_solved = if opts.top >= 1 {
        tr.e().min(opts.top - 1)
    } else {
        tr.e()
    };
    let (mut f, v, n) = if tr.s() + tr.e() >= 1 {
        let mut series = GnIncremental::new(&sys);
        for tot in 1..=(tr.s() + tr.e()) {
            let eps = eps_solved.min(tot);
            if tot > tr.s() + eps {
                continue;
            }
            let g = series.evaluate_g(&sx, tr.computing(tot, eps))?;
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
        series.finish_vn(&sx, tr)?
    } else {
        let (_, f, v, n) = gn_evaluate_vn(&sys, &sx, tr)?;
        (f, v, n)
    };
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
    use super::super::series::{MAX_E, STORE_E};
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
    fn seeded_newton_residual_matches_the_point_evaluation() {
        let (n, u, v, ku, kv) = cylinder_at();
        let curved = nurbs_partials::<f64>(&n, ku, kv, u, v, 2);
        let flat = surface_partials(
            &Kernel::default(),
            &plane(Vec3::X, Vec3::Y),
            Point2::new(0.4, 0.7),
            2,
        )
        .unwrap();
        let sx = [u, v, 0.4, 0.7].map(Ser::constant);
        let law = Ser::constant(0.85);
        for (law, equal_setback) in [(None, None), (Some(&law), None), (None, Some(0.75))] {
            let sys = GnSystem {
                p: [&curved, &flat],
                leader: VSer::constant(V3 {
                    x: 0.3,
                    y: 0.4,
                    z: 0.5,
                }),
                tangent: VSer::constant(V3 {
                    x: 0.0,
                    y: 1.0,
                    z: 0.0,
                }),
                r: [9.99, 1.25],
                side: [-1.0, 1.0],
                hold: false,
                law,
                equal_setback,
            };
            let (point, _) = gn_evaluate(&sys, &sx, Trunc::new(0, 0).unwrap()).unwrap();
            let (residual, _) = gn_linearize(&sys, &sx).unwrap();
            assert_eq!(
                residual.map(f64::to_bits),
                point.map(|g| g.at(0, 0).to_bits())
            );
        }
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

    #[test]
    fn partials_with_order_matches_default_and_grows() {
        let mut small = Partials::<Ivl>::with_order(2);
        let mut full = Partials::<Ivl>::default();
        let v = V3 {
            x: Ivl::new(1.0, 2.0),
            y: Ivl::new(-3.0, 4.0),
            z: Ivl::from(0.5),
        };
        small.set(1, 1, v);
        full.set(1, 1, v);
        assert_eq!(small, full);
        assert_eq!(small.at(3, 0), full.at(3, 0));
        // Writing past the initial order grows the table transparently.
        small.set(4, 2, v);
        full.set(4, 2, v);
        assert_eq!(small, full);
        assert_eq!(small.at(4, 2), v);
    }

    #[test]
    fn point_and_dual_evaluators_match_series_bit_for_bit() {
        fn check_scalar<T: GnScalar + PartialEq + core::fmt::Debug>(
            mk: impl Fn(f64) -> T,
            r: [f64; 2],
            law: bool,
            equal_setback: Option<f64>,
        ) {
            let mut seed = 0x517c_c1b7_2722_0a95_u64;
            let mut next = move || {
                seed = seed
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1_442_695_040_888_963_407);
                (seed >> 11) as f64 / (1_u64 << 53) as f64 - 0.5
            };
            let mut v3 = |base: [f64; 3]| V3 {
                x: mk(base[0] + 0.25 * next()),
                y: mk(base[1] + 0.25 * next()),
                z: mk(base[2] + 0.25 * next()),
            };
            let mut faces = [Partials::with_order(2), Partials::with_order(2)];
            for (f, pf) in faces.iter_mut().enumerate() {
                let sgn = if f == 0 { 1.0 } else { -1.0 };
                for a in 0..=2 {
                    for b in 0..=(2 - a) {
                        let base = match (a, b) {
                            (1, 0) => [1.0, 0.0, 0.0],
                            (0, 1) => [0.0, sgn, 0.2],
                            _ => [0.0; 3],
                        };
                        pf.set(a, b, v3(base));
                    }
                }
            }
            let leader = VSer::constant(v3([0.1, 0.2, 0.3]));
            let tangent = VSer::constant(v3([0.0, 0.6, 0.8]));
            let law_ser = Ser::constant(mk(0.85));
            let [p0, p1] = &faces;
            let sys = GnSystem {
                p: [p0, p1],
                leader,
                tangent,
                r,
                side: [1.0, -1.0],
                hold: false,
                law: law.then_some(&law_ser),
                equal_setback,
            };
            let sx = [0.1, 0.2, 0.3, 0.4].map(|c| Ser::constant(mk(c)));
            // Point evaluation (Trunc(0, 0)): exact match with gn_evaluate.
            let (g_ref, _) = gn_evaluate(&sys, &sx, Trunc::new(0, 0).unwrap()).unwrap();
            let g_pt = gn_evaluate_point_ref(
                sys.p,
                &sys.leader,
                &sys.tangent,
                sys.r,
                sys.side,
                sys.law,
                sys.equal_setback,
            )
            .unwrap();
            assert_eq!(g_pt, g_ref.each_ref().map(|gi| gi.at(0, 0)));

            // Linearize / Jacobian (Trunc(0, 1), hold = true): exact match with gn_evaluate.
            let mut held = sys.clone();
            held.hold = true;
            let tr01 = Trunc::new(0, 1).unwrap();
            let mut want_res = [T::of(0.0); 4];
            let mut want_jac = [[T::of(0.0); 4]; 4];
            for j in 0..4 {
                let mut sxj = sx.clone();
                sxj[j].set(0, 1, T::of(1.0));
                let (gj, _) = gn_evaluate(&held, &sxj, tr01).unwrap();
                if j == 0 {
                    want_res = gj.each_ref().map(|gi| gi.at(0, 0));
                }
                for row in 0..4 {
                    want_jac[row][j] = gj[row].at(0, 1);
                }
            }
            let (got_res, got_jac) = gn_linearize_ref(
                sys.p,
                &sys.leader,
                &sys.tangent,
                sys.r,
                sys.side,
                sys.law,
                sys.equal_setback,
            )
            .unwrap();
            assert_eq!(got_res, want_res);
            assert_eq!(got_jac, want_jac);
        }
        let tm = |v: f64| {
            crate::blend::fillet_section::feet_tm::Ft::from_points(&[v, 0.01 * v, -0.004 * v])
        };
        let iv = |v: f64| Ivl::new(v - 1e-4, v + 1e-4);
        for (r, law, eq) in [
            ([0.5, 0.75], false, None),
            ([8.0, 12.0], false, None),
            ([0.5, 0.75], true, None),
            ([8.0, 12.0], true, None),
            ([0.5, 0.75], false, Some(0.4)),
        ] {
            check_scalar(|v| v, r, law, eq);
            check_scalar(iv, r, law, eq);
            check_scalar(tm, r, law, eq);
        }
    }

    #[test]
    fn incremental_evaluate_g_and_into_fv_match_whole_bit_for_bit() {
        fn run<T: GnScalar + PartialEq + core::fmt::Debug>(
            mk: impl Fn(f64) -> T,
            s: usize,
            e: usize,
            n: usize,
            r: [f64; 2],
        ) {
            let mut seed = 0x1234_5678_9abc_def0_u64;
            let mut next = move || {
                seed = seed
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1_442_695_040_888_963_407);
                (seed >> 11) as f64 / (1_u64 << 53) as f64 - 0.5
            };
            let mut v3 = |base: [f64; 3]| V3 {
                x: mk(base[0] + 0.25 * next()),
                y: mk(base[1] + 0.25 * next()),
                z: mk(base[2] + 0.25 * next()),
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
                for i in 0..=s {
                    x.set(i, 0, mk(if i == 0 { c0 } else { 0.1 * next() }));
                }
                x
            };
            let [p0, p1] = &faces;
            let sys = GnSystem {
                p: [p0, p1],
                leader: VSer {
                    a: [ser(0.1), ser(0.2), ser(0.3)],
                },
                tangent: VSer {
                    a: [ser(0.0), ser(0.6), ser(0.8)],
                },
                r,
                side: [1.0, -1.0],
                hold: false,
                law: None,
                equal_setback: None,
            };
            let tr = Trunc::new(s, e).unwrap();
            let mut sx = [0.3, -0.2, 0.1, 0.4].map(|c| Ser::constant(mk(c)));
            let mut inc = GnIncremental::new(&sys);
            let eps_solved = if n >= 1 { e.min(n - 1) } else { e };
            for tot in 1..=(s + e) {
                let eps = eps_solved.min(tot);
                if tot > s + eps {
                    continue;
                }
                let at = tr.computing(tot, eps);
                let (want_g, _, _, _) = gn_evaluate_vn(&sys, &sx, at).unwrap();
                let got_g = inc.evaluate_g(&sx, at).unwrap();
                for i in 0..=s.min(tot) {
                    let kk = tot - i;
                    if kk <= eps {
                        for q in 0..4 {
                            assert_eq!(got_g[q].at(i, kk), want_g[q].at(i, kk));
                        }
                    }
                }
                for i in 0..=s.min(tot) {
                    let kk = tot - i;
                    if kk > e || (n >= 1 && kk >= n) {
                        continue;
                    }
                    for xc in &mut sx {
                        xc.set(i, kk, mk(0.15 * next()));
                    }
                }
            }
            let need_top = n >= 1 && n <= e;
            inc.evaluate_final(&sx, tr, need_top).unwrap();
            let (mut got_f, got_v, got_n) = inc.into_fv(tr, need_top);
            if let Some(nrm) = got_n {
                gn_top_jets(n, tr, &nrm, &mut got_f);
            }
            let (_, mut want_f, want_v, want_n) = gn_evaluate_vn(&sys, &sx, tr).unwrap();
            gn_top_jets(n, tr, &want_n, &mut want_f);
            assert_eq!(got_f, want_f);
            assert_eq!(got_v, want_v);
        }
        let tm = |v: f64| {
            crate::blend::fillet_section::feet_tm::Ft::from_points(&[v, 0.01 * v, -0.005 * v])
        };
        for (s, e, n) in [(4, 0, 0), (3, 2, 2), (2, 3, 3)] {
            for r in [[0.5, 0.75], [8.0, 12.0]] {
                run(|v| v, s, e, n, r);
                run(Ivl::from, s, e, n, r);
                run(tm, s, e, n, r);
            }
        }
    }
}
