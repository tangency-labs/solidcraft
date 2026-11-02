//! The Mixed corner's exact piecewise-Bezier algebra in homogeneous coordinates
//! (`x w, y w, z w, w`), and its faces' bicubically blended Coons patches in R^4.
//!
//! Upstream: none (pinned SMLib has no Mixed corner). The fork's
//! `source/SMLib/src/SmBlendCornerMixed.cpp` (tangency-labs/smlib d443f10, branch
//! `wip/82-mixed-corner`; smlib#82), its anonymous namespace. Every operation here is
//! exact to rounding: knot insertion, de Casteljau subdivision, degree elevation,
//! Bernstein products, sums, and the composition of a rational surface with a curve in
//! its parameters. A curve is a list of Bezier segments of one degree over its breaks.
//!
//! | Rust                         | Fork                                  |
//! |------------------------------|---------------------------------------|
//! | [`V4`], [`Bez`], [`Crv`], [`Srf`] | `SmMxV4`, `SmMxBez`, `SmMxCrv`, `SmMxSrf` |
//! | [`add`], [`sub`], [`mul`], [`lerp`], [`point`], [`dir`], [`project`] | `sm_MxAdd`, `sm_MxSub`, `sm_MxMul`, `sm_MxLerp`, `sm_MxPoint`, `sm_MxDir`, `sm_MxProject` |
//! | `choose` (private)           | `sm_MxChoose`                         |
//! | `bez_eval`, `bez_split`, `bez_elevate`, `bez_deriv`, `bez_times` (private) | `sm_MxBezEval`, `sm_MxBezSplit`, `sm_MxBezElevate`, `sm_MxBezDeriv`, `sm_MxBezTimes` |
//! | [`eval`], [`elevate`], [`refine`], `union` (private), [`common`], [`sum`], [`times`], [`deriv`], [`map`] | `sm_MxEval`, `sm_MxElevate`, `sm_MxRefine`, `sm_MxUnion`, `sm_MxCommon`, `sm_MxSum`, `sm_MxTimes`, `sm_MxDeriv`, `sm_MxMap` |
//! | `hermite` (private), [`constant`], [`unit`], [`unit_weights`], [`c1`] | `sm_MxHermite`, `sm_MxConst`, `sm_MxUnit`, `sm_MxUnitWeights`, `sm_MxC1` |
//! | [`from_bspline`], [`srf_from_bspline`] | `sm_MxFromBSpline`, `sm_MxSrfFromBSpline` |
//! | `roots` (private), [`compose`], [`srf_deriv_v`] | `sm_MxRoots`, `sm_MxCompose`, `sm_MxSrfDerivV` |
//! | `solve` (private)            | `sm_MxSolve`                          |
//! | [`Side`], [`side_ends`], [`crv_deriv_at`], [`first_order`], [`TwistTerm`], [`twists`], `ribbon` (private) | `SmMxSide`, `sm_MxSideEnds`, `sm_MxCrvDerivAt`, `sm_MxFirstOrder`, `SmMxTwistTerm`, `sm_MxTwists`, `sm_MxRibbon` |
//! | `srf_map_u`, [`srf_transpose`], `srf_map_v`, `srf_common`, `tensor_u`, `srf_sum` (private) | `sm_MxSrfMapU`, `sm_MxSrfTranspose`, `sm_MxSrfMapV`, `sm_MxSrfCommon`, `sm_MxTensorU`, `sm_MxSrfSum` |
//! | [`coons`], [`coons_bilinear`], [`srf_to_bspline`] | `sm_MxCoons`, `sm_MxCoonsBilinear`, `sm_MxSrfToBSpline` |
//! | [`piece`], [`to_uv`]         | `sm_MxPiece`, `sm_MxToUV`             |
//!
//! Deviations: out-params are return values; an index the fork reads out of range (only
//! on malformed data) is `SM_ERR`; a twist term names its side by an index into the
//! caller's sides instead of a pointer. `sm_MxRoots` recurses at most 61 deep (its own
//! `depth > 60` stop, as the fork). Opt-in with the Mixed corner
//! (`Kernel::fixes.fillet_blend_corner`, WR-697).

use crate::error::{Result, SolidError};
use crate::math::{Point3, Vec3, BIG_DOUBLE};
use crate::nurbs::b_spline_curve::p1::{BSplineCurveForm, CanonicalData, KnotType};
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::b_spline_surface::p1::{BSplineSurfaceForm, CanonicalSurfaceData};
use crate::nurbs::b_spline_surface::BSplineSurface;

const OP: &str = "blend::blend_corner::mixed::algebra";

/// `SmMxV4`: a homogeneous point or direction (`x w, y w, z w, w`).
pub type V4 = [f64; 4];
/// `SmMxBez`: a Bezier segment on `[0, 1]` (degree = length - 1).
pub type Bez = Vec<V4>;

/// `SmMxCrv`: a piecewise Bezier curve, segment `s` on `[brk[s], brk[s + 1]]`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Crv {
    /// `brk`: the breaks.
    pub brk: Vec<f64>,
    /// `seg`: the segments, all of one degree.
    pub seg: Vec<Bez>,
}

impl Crv {
    /// `Deg()`: the segments' degree (`None` for no segment, the fork's -1).
    pub fn deg(&self) -> Option<usize> {
        self.seg.first().map(|s| s.len().saturating_sub(1))
    }
}

/// `SmMxSrf`: a surface's Bezier patches, patch `(i, j)` over `[ub[i], ub[i+1]] x
/// [vb[j], vb[j+1]]` at `p[i * (vb.len() - 1) + j]`, control point `(a, b)` at
/// `a * (dv + 1) + b`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Srf {
    /// `du`.
    pub du: usize,
    /// `dv`.
    pub dv: usize,
    /// `ub`.
    pub ub: Vec<f64>,
    /// `vb`.
    pub vb: Vec<f64>,
    /// `P`.
    pub p: Vec<Vec<V4>>,
}

impl Srf {
    /// `At(i, j)`.
    fn at(&self, i: usize, j: usize) -> Result<&Vec<V4>> {
        let nvs = self.vb.len().checked_sub(1).ok_or_else(err)?;
        g(&self.p, i * nvs + j)
    }
}

fn err() -> SolidError {
    SolidError::sm_err(OP)
}

fn g<T>(v: &[T], i: usize) -> Result<&T> {
    v.get(i).ok_or_else(err)
}

fn gm<T>(v: &mut [T], i: usize) -> Result<&mut T> {
    v.get_mut(i).ok_or_else(err)
}

fn first<T: Copy>(v: &[T]) -> Result<T> {
    v.first().copied().ok_or_else(err)
}

fn last<T: Copy>(v: &[T]) -> Result<T> {
    v.last().copied().ok_or_else(err)
}

/// `std::min` (`(b < a) ? b : a`).
fn min(a: f64, b: f64) -> f64 {
    if b < a {
        b
    } else {
        a
    }
}

/// `std::max` (`(a < b) ? b : a`).
fn max(a: f64, b: f64) -> f64 {
    if a < b {
        b
    } else {
        a
    }
}

/// `sm_MxAdd`.
#[must_use]
pub fn add(a: V4, b: V4) -> V4 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2], a[3] + b[3]]
}

/// `sm_MxSub`.
#[must_use]
pub fn sub(a: V4, b: V4) -> V4 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2], a[3] - b[3]]
}

/// `sm_MxMul`.
#[must_use]
pub fn mul(a: V4, s: f64) -> V4 {
    [a[0] * s, a[1] * s, a[2] * s, a[3] * s]
}

/// `sm_MxLerp`.
#[must_use]
pub fn lerp(a: V4, b: V4, t: f64) -> V4 {
    add(mul(a, 1.0 - t), mul(b, t))
}

/// `sm_MxPoint`: a point at weight `w`.
#[must_use]
pub fn point(p: Point3, w: f64) -> V4 {
    [p.x * w, p.y * w, p.z * w, w]
}

/// `sm_MxDir`.
#[must_use]
pub fn dir(d: Vec3) -> V4 {
    [d.x, d.y, d.z, 0.0]
}

/// `sm_MxProject`.
#[must_use]
pub fn project(a: V4) -> Point3 {
    Point3::new(a[0] / a[3], a[1] / a[3], a[2] / a[3])
}

/// `sm_MxChoose`.
fn choose(n: usize, k: usize) -> f64 {
    let mut d = 1.0;
    for i in 1..=k {
        d = d * (n - k + i) as f64 / i as f64;
    }
    d
}

/// `sm_MxBezEval`.
fn bez_eval(b: &[V4], t: f64) -> Result<V4> {
    let mut c = b.to_vec();
    for r in 1..c.len() {
        for i in 0..c.len() - r {
            let next = *g(&c, i + 1)?;
            let cur = gm(&mut c, i)?;
            *cur = lerp(*cur, next, t);
        }
    }
    first(&c)
}

/// `sm_MxBezSplit`: the segment's halves at `t`.
fn bez_split(b: &[V4], t: f64) -> Result<(Bez, Bez)> {
    let n = b.len();
    let mut c = b.to_vec();
    let mut l = vec![V4::default(); n];
    let mut r = vec![V4::default(); n];
    let last_i = n.checked_sub(1).ok_or_else(err)?;
    *gm(&mut l, 0)? = *g(&c, 0)?;
    *gm(&mut r, last_i)? = *g(&c, last_i)?;
    for k in 1..n {
        for i in 0..n - k {
            let next = *g(&c, i + 1)?;
            let cur = gm(&mut c, i)?;
            *cur = lerp(*cur, next, t);
        }
        *gm(&mut l, k)? = *g(&c, 0)?;
        *gm(&mut r, n - 1 - k)? = *g(&c, n - 1 - k)?;
    }
    Ok((l, r))
}

/// `sm_MxBezElevate`.
fn bez_elevate(b: &[V4], to: usize) -> Result<Bez> {
    let mut c = b.to_vec();
    if c.is_empty() {
        return Err(err());
    }
    while c.len() - 1 < to {
        let n = c.len() - 1;
        let mut e = vec![V4::default(); n + 2];
        *gm(&mut e, 0)? = *g(&c, 0)?;
        *gm(&mut e, n + 1)? = *g(&c, n)?;
        for i in 1..=n {
            *gm(&mut e, i)? = add(
                mul(*g(&c, i - 1)?, i as f64 / (n + 1) as f64),
                mul(*g(&c, i)?, 1.0 - i as f64 / (n + 1) as f64),
            );
        }
        c = e;
    }
    Ok(c)
}

/// `sm_MxBezDeriv`: d/dt of a segment over a span `len` (a Bezier of one degree less).
fn bez_deriv(b: &[V4], len: f64) -> Result<Bez> {
    let n = b.len().checked_sub(1).ok_or_else(err)?;
    if n == 0 {
        return Ok(vec![V4::default()]);
    }
    let mut d = Vec::new();
    for i in 0..n {
        d.push(mul(sub(*g(b, i + 1)?, *g(b, i)?), n as f64 / len));
    }
    Ok(d)
}

/// `sm_MxBezTimes`: the product of a scalar Bezier (its first coordinates) and a Bezier.
fn bez_times(s: &[V4], p: &[V4]) -> Result<Bez> {
    let m = s.len().checked_sub(1).ok_or_else(err)?;
    let n = p.len().checked_sub(1).ok_or_else(err)?;
    let mut r = vec![V4::default(); m + n + 1];
    for i in 0..=m {
        for j in 0..=n {
            let term = mul(*g(p, j)?, g(s, i)?[0] * choose(m, i) * choose(n, j));
            let slot = gm(&mut r, i + j)?;
            *slot = add(*slot, term);
        }
    }
    for k in 0..=m + n {
        let slot = gm(&mut r, k)?;
        *slot = mul(*slot, 1.0 / choose(m + n, k));
    }
    Ok(r)
}

/// `sm_MxEval`.
pub fn eval(c: &Crv, t: f64) -> Result<V4> {
    let mut s = 0;
    while s + 1 < c.seg.len() && t > *g(&c.brk, s + 1)? {
        s += 1;
    }
    let a = *g(&c.brk, s)?;
    let b = *g(&c.brk, s + 1)?;
    bez_eval(g(&c.seg, s)?, (t - a) / (b - a))
}

/// `sm_MxElevate`.
pub fn elevate(c: &Crv, to: usize) -> Result<Crv> {
    let mut r = c.clone();
    for s in &mut r.seg {
        *s = bez_elevate(s, to)?;
    }
    Ok(r)
}

/// `sm_MxRefine`: `c`'s segments split at every break of `brks` inside them.
pub fn refine(c: &Crv, brks: &[f64]) -> Result<Crv> {
    let mut r = Crv {
        brk: vec![first(&c.brk)?],
        seg: Vec::new(),
    };
    for (s, seg) in c.seg.iter().enumerate() {
        let a = *g(&c.brk, s)?;
        let b = *g(&c.brk, s + 1)?;
        let mut rest = seg.clone();
        let mut from = a;
        for &t in brks {
            if t <= from + 1e-14 * (1.0 + t.abs()) || t >= b - 1e-14 * (1.0 + t.abs()) {
                continue;
            }
            let (l, rr) = bez_split(&rest, (t - from) / (b - from))?;
            r.seg.push(l);
            r.brk.push(t);
            rest = rr;
            from = t;
        }
        r.seg.push(rest);
        r.brk.push(b);
    }
    Ok(r)
}

/// `sm_MxUnion`: the sorted breaks of both, those within rounding of the previous one
/// dropped.
fn union(a: &[f64], b: &[f64]) -> Vec<f64> {
    let mut u = a.to_vec();
    u.extend_from_slice(b);
    u.sort_by(|x, y| x.partial_cmp(y).unwrap_or(core::cmp::Ordering::Equal));
    let mut r: Vec<f64> = Vec::new();
    for t in u {
        match r.last() {
            Some(&back) if t <= back + 1e-13 * (1.0 + t.abs()) => {}
            _ => r.push(t),
        }
    }
    r
}

/// `sm_MxCommon`: `a` and `b` on common breaks and degree.
pub fn common(a: &mut Crv, b: &mut Crv) -> Result<()> {
    let u = union(&a.brk, &b.brk);
    *a = refine(a, &u)?;
    *b = refine(b, &u)?;
    let d = a.deg().max(b.deg()).ok_or_else(err)?;
    *a = elevate(a, d)?;
    *b = elevate(b, d)?;
    // the breaks as one list (rounding apart only)
    b.brk.clone_from(&a.brk);
    Ok(())
}

/// `sm_MxSum`: `sa a + sb b`.
pub fn sum(a: &Crv, b: &Crv, sa: f64, sb: f64) -> Result<Crv> {
    let mut a = a.clone();
    let mut b = b.clone();
    common(&mut a, &mut b)?;
    for (s, seg) in a.seg.iter_mut().enumerate() {
        let other = g(&b.seg, s)?;
        for (i, q) in seg.iter_mut().enumerate() {
            *q = add(mul(*q, sa), mul(*g(other, i)?, sb));
        }
    }
    Ok(a)
}

/// `sm_MxTimes`: `s` (a scalar curve) times `p`.
pub fn times(s: &Crv, p: &Crv) -> Result<Crv> {
    let u = union(&s.brk, &p.brk);
    let s = refine(s, &u)?;
    let p = refine(p, &u)?;
    let mut r = Crv {
        brk: s.brk.clone(),
        seg: Vec::new(),
    };
    for (k, seg) in s.seg.iter().enumerate() {
        r.seg.push(bez_times(seg, g(&p.seg, k)?)?);
    }
    Ok(r)
}

/// `sm_MxDeriv`.
pub fn deriv(c: &Crv) -> Result<Crv> {
    let mut r = Crv {
        brk: c.brk.clone(),
        seg: Vec::new(),
    };
    for (s, seg) in c.seg.iter().enumerate() {
        r.seg
            .push(bez_deriv(seg, *g(&c.brk, s + 1)? - *g(&c.brk, s)?)?);
    }
    Ok(r)
}

/// `sm_MxMap`: `c` with every control point mapped by `f` (a linear map of R^4).
pub fn map(c: &Crv, f: impl Fn(V4) -> V4) -> Crv {
    let mut r = c.clone();
    for b in &mut r.seg {
        for p in b.iter_mut() {
            *p = f(*p);
        }
    }
    r
}

/// `sm_MxHermite`: the scalar cubic Hermite on `[0, 1]`: values `v0`, `v1`,
/// derivatives `d0`, `d1`.
fn hermite(v0: f64, v1: f64, d0: f64, d1: f64) -> Crv {
    Crv {
        brk: vec![0.0, 1.0],
        seg: vec![vec![
            [v0, 0.0, 0.0, 0.0],
            [v0 + d0 / 3.0, 0.0, 0.0, 0.0],
            [v1 - d1 / 3.0, 0.0, 0.0, 0.0],
            [v1, 0.0, 0.0, 0.0],
        ]],
    }
}

/// `sm_MxConst`.
#[must_use]
pub fn constant(v: V4) -> Crv {
    Crv {
        brk: vec![0.0, 1.0],
        seg: vec![vec![v]],
    }
}

/// `sm_MxUnit`: `c` on `[0, 1]` (linearly), or reversed.
pub fn unit(c: &Crv, reverse: bool) -> Result<Crv> {
    let a = first(&c.brk)?;
    let b = last(&c.brk)?;
    let n = c.seg.len();
    let mut r = Crv::default();
    for i in 0..=n {
        let t = (*g(&c.brk, if reverse { n - i } else { i })? - a) / (b - a);
        r.brk.push(if reverse { 1.0 - t } else { t });
    }
    for i in 0..n {
        let mut s = g(&c.seg, if reverse { n - 1 - i } else { i })?.clone();
        if reverse {
            s.reverse();
        }
        r.seg.push(s);
    }
    *r.brk.first_mut().ok_or_else(err)? = 0.0;
    *r.brk.last_mut().ok_or_else(err)? = 1.0;
    Ok(r)
}

/// `sm_MxUnitWeights`: `c` scaled by the cubic factor rho that makes its weight 1 with
/// zero derivative at both ends (the same curve, the same parameterization). A
/// polynomial curve is left as it is; a rho not positive on the whole interval (seen
/// on its Bernstein coefficients), or a curve of degree above 10, takes the linear
/// factor, its end weights 1.
pub fn unit_weights(c: &Crv) -> Result<Crv> {
    let a = first(&c.brk)?;
    let b = last(&c.brk)?;
    let l = b - a;
    let d = deriv(c)?;
    let w0 = eval(c, a)?[3];
    let w1 = eval(c, b)?[3];
    let dw0 = eval(&d, a)?[3];
    let dw1 = eval(&d, b)?[3];
    if (w0 - 1.0).abs() < 1e-15
        && (w1 - 1.0).abs() < 1e-15
        && dw0.abs() < 1e-15
        && dw1.abs() < 1e-15
    {
        return Ok(c.clone());
    }
    // rho on [a, b] as a cubic: values 1/w, derivatives -w'/w^2 (per unit of t)
    let r0 = 1.0 / w0;
    let r1 = 1.0 / w1;
    let s0 = -dw0 / (w0 * w0) * l;
    let s1 = -dw1 / (w1 * w1) * l;
    let q = [r0, r0 + s0 / 3.0, r1 - s1 / 3.0, r1];
    let mut f = Crv {
        brk: vec![a, b],
        seg: Vec::new(),
    };
    // (a curve of degree above 10 takes the linear factor: its ribbon, three degrees
    // above it with the cubic, keeps within the B-spline degree limit, 17)
    if q[1] > 0.0 && q[2] > 0.0 && c.deg().is_some_and(|d| d <= 10) {
        f.seg.push(q.iter().map(|&x| [x, 0.0, 0.0, 0.0]).collect());
    } else {
        f.seg.push(vec![[r0, 0.0, 0.0, 0.0], [r1, 0.0, 0.0, 0.0]]);
    }
    times(&f, c)
}

/// `sm_MxC1`: `c` (and its companions on the same breaks: the curve in the parameters
/// `c` is the image of, and a field along `c` of `c`'s degree, which then keeps its own
/// continuity) made C1 in R^4 at every joint, the same curves: at a joint Q (weight 1)
/// with left derivative `D_L = l D0 + m Q`, `D0 = n (R_1 - R_0)` the right segment's
/// own, the right segment's control points are scaled by `k^i` (a Moebius
/// reparametrization) on a span `h = n / (n l - m)`, `k = l h`, the segments after it
/// by `k^n`; the breaks are then rescaled to `c`'s interval.
pub fn c1(c: &mut Crv, companions: &mut [&mut Crv]) -> Result<()> {
    for comp in companions.iter() {
        if comp.seg.len() != c.seg.len() {
            return Err(err());
        }
    }
    let n = c.deg().ok_or_else(err)?;
    let a0 = first(&c.brk)?;
    let a1 = last(&c.brk)?;
    let mut h = Vec::new();
    for s in 0..c.seg.len() {
        h.push(*g(&c.brk, s + 1)? - *g(&c.brk, s)?);
    }
    for s in 1..c.seg.len() {
        let left = g(&c.seg, s - 1)?.clone();
        let nm1 = n.checked_sub(1).ok_or_else(err)?;
        let dl = mul(
            sub(*g(&left, n)?, *g(&left, nm1)?),
            n as f64 / *g(&h, s - 1)?,
        );
        let right = gm(&mut c.seg, s)?;
        let q = *g(right, 0)?;
        let d0 = mul(sub(*g(right, 1)?, *g(right, 0)?), n as f64);
        // D_L = l D0 + m Q, least squares in R^4
        let (mut aa, mut ab, mut bb, mut ar, mut br) = (0.0, 0.0, 0.0, 0.0, 0.0);
        for ((&dk, &qk), &lk) in d0.iter().zip(q.iter()).zip(dl.iter()) {
            aa += dk * dk;
            ab += dk * qk;
            bb += qk * qk;
            ar += dk * lk;
            br += qk * lk;
        }
        let det = aa * bb - ab * ab;
        if det.is_nan() || det <= 0.0 {
            return Err(err());
        }
        let l = (ar * bb - ab * br) / det;
        let m = (aa * br - ab * ar) / det;
        let den = n as f64 * l - m;
        if den.is_nan() || den <= 0.0 {
            return Err(err());
        }
        let hs = n as f64 / (n as f64 * l - m);
        let k = l * hs;
        let mut f = 1.0;
        for p in right.iter_mut().take(n + 1) {
            *p = mul(*p, f);
            f *= k;
        }
        for comp in companions.iter_mut() {
            f = 1.0;
            for q in gm(&mut comp.seg, s)?.iter_mut() {
                *q = mul(*q, f);
                f *= k;
            }
        }
        let kn = k.powf(n as f64);
        for t in s + 1..c.seg.len() {
            for q in gm(&mut c.seg, t)?.iter_mut() {
                *q = mul(*q, kn);
            }
            for comp in companions.iter_mut() {
                let kd = k.powf(comp.deg().ok_or_else(err)? as f64);
                for q in gm(&mut comp.seg, t)?.iter_mut() {
                    *q = mul(*q, kd);
                }
            }
        }
        *gm(&mut h, s)? = hs;
    }
    let mut d_sum = 0.0;
    for &x in &h {
        d_sum += x;
    }
    let mut t = a0;
    for (s, &hs) in h.iter().enumerate() {
        *gm(&mut c.brk, s)? = t;
        t += hs * (a1 - a0) / d_sum;
    }
    *c.brk.last_mut().ok_or_else(err)? = a1;
    for comp in companions.iter_mut() {
        comp.brk.clone_from(&c.brk);
    }
    Ok(())
}

/// The Boehm insertion of every interior knot up to multiplicity `p` of the B-spline
/// of degree `p` on the unique knots `knots` (multiplicities `mults`) and the
/// homogeneous control points `pts`: its Bezier segments (the shared body of
/// `sm_MxFromBSpline` and `sm_MxSrfFromBSpline`'s `toCrv`).
fn boehm(pts: Vec<V4>, p: usize, mults: &[usize], knots: &[f64]) -> Result<Crv> {
    let mut u: Vec<f64> = Vec::new();
    for (i, &t) in knots.iter().enumerate() {
        for _ in 0..*g(mults, i)? {
            u.push(t);
        }
    }
    let mut pp = pts;
    for ki in 1..knots.len().saturating_sub(1) {
        let t = *g(knots, ki)?;
        for _ in *g(mults, ki)?..p {
            let mut k = 0;
            while k + 1 < u.len() && *g(&u, k + 1)? <= t {
                k += 1;
            }
            // t in [U[k], U[k+1]), s = current multiplicity
            let s = u.iter().filter(|&&x| x == t).count();
            let mut q = vec![V4::default(); pp.len() + 1];
            let kp = k.checked_sub(p).ok_or_else(err)?;
            for i in 0..=kp {
                *gm(&mut q, i)? = *g(&pp, i)?;
            }
            let ks = (k + 1).checked_sub(s).ok_or_else(err)?;
            for i in ks..=pp.len() {
                *gm(&mut q, i)? = *g(&pp, i - 1)?;
            }
            for i in kp + 1..ks {
                let a = (t - *g(&u, i)?) / (*g(&u, i + p)? - *g(&u, i)?);
                *gm(&mut q, i)? = lerp(*g(&pp, i - 1)?, *g(&pp, i)?, a);
            }
            u.insert(k + 1, t);
            pp = q;
        }
    }
    let mut out = Crv {
        brk: knots.to_vec(),
        seg: Vec::new(),
    };
    for s in 0..knots.len().saturating_sub(1) {
        out.seg
            .push(pp.get(s * p..s * p + p + 1).ok_or_else(err)?.to_vec());
    }
    Ok(out)
}

/// `sm_MxFromBSpline`: the B-spline curve's segments by knot insertion (homogeneous; a
/// 2D curve's z is 0).
pub fn from_bspline(curve: &BSplineCurve) -> Result<Crv> {
    let can = curve.canonical()?;
    let p = can.degree;
    let mut u_len = 0usize;
    for &m in &can.knot_multiplicities {
        u_len += m;
    }
    let mut pts = Vec::new();
    for (i, &pt) in can.control_points.iter().enumerate() {
        let w = if can.weights.is_empty() {
            1.0
        } else {
            *g(&can.weights, i)?
        };
        pts.push(point(pt, w));
    }
    if u_len != pts.len() + p + 1 {
        return Err(err());
    }
    boehm(pts, p, &can.knot_multiplicities, &can.knots)
}

/// `sm_MxSrfFromBSpline`: a surface's Bezier patches (each row a curve in v made
/// piecewise Bezier, then each column in u).
pub fn srf_from_bspline(surf: &BSplineSurface) -> Result<Srf> {
    let can = surf.canonical()?;
    let (du, dv) = (can.u_degree, can.v_degree);
    let nu = can
        .u_multiplicities
        .iter()
        .sum::<usize>()
        .checked_sub(du + 1)
        .ok_or_else(err)?;
    let nv = can
        .v_multiplicities
        .iter()
        .sum::<usize>()
        .checked_sub(dv + 1)
        .ok_or_else(err)?;
    if nu * nv != can.control_points.len() {
        return Err(err());
    }
    let weight = |i: usize| -> Result<f64> {
        if can.weights.is_empty() {
            Ok(1.0)
        } else {
            g(&can.weights, i).copied()
        }
    };
    let mut rows = Vec::new();
    for i in 0..nu {
        let mut row = Vec::new();
        for j in 0..nv {
            row.push(point(
                *g(&can.control_points, i * nv + j)?,
                weight(i * nv + j)?,
            ));
        }
        rows.push(boehm(row, dv, &can.v_multiplicities, &can.v_knots)?);
    }
    let row0 = g(&rows, 0)?;
    let nvs = row0.seg.len();
    let mut out = Srf {
        du,
        dv,
        vb: row0.brk.clone(),
        ub: can.u_knots.clone(),
        p: Vec::new(),
    };
    let nus = out.ub.len().checked_sub(1).ok_or_else(err)?;
    out.p = vec![vec![V4::default(); (du + 1) * (dv + 1)]; nus * nvs];
    for js in 0..nvs {
        for b in 0..=dv {
            let mut col = Vec::new();
            for row in &rows {
                col.push(*g(g(&row.seg, js)?, b)?);
            }
            let c = boehm(col, du, &can.u_multiplicities, &can.u_knots)?;
            for is in 0..nus {
                for a in 0..=du {
                    *gm(gm(&mut out.p, is * nvs + js)?, a * (dv + 1) + b)? = *g(g(&c.seg, is)?, a)?;
                }
            }
        }
    }
    Ok(out)
}

/// `sm_MxRoots`: the parameters in `(0, 1)` where a scalar Bezier crosses `level`
/// (subdivision, then bisection to rounding), mapped to `[a, b]`. At most 61 deep.
fn roots(bz: &[f64], level: f64, a: f64, b: f64, depth: u32, out: &mut Vec<f64>) -> Result<()> {
    let mut lo = BIG_DOUBLE;
    let mut hi = -BIG_DOUBLE;
    for &c in bz {
        lo = min(lo, c - level);
        hi = max(hi, c - level);
    }
    if lo > 0.0 || hi < 0.0 {
        return Ok(());
    }
    if depth > 60 || b - a < 1e-15 {
        out.push(0.5 * (a + b));
        return Ok(());
    }
    let fa = first(bz)? - level;
    let fb = last(bz)? - level;
    // monotone control polygon with a sign change: bisect on the Bezier
    let mut mono = true;
    for i in 1..bz.len() {
        if !mono {
            break;
        }
        if (*g(bz, i)? - *g(bz, i - 1)?) * (fb - fa) < 0.0 {
            mono = false;
        }
    }
    if mono && fa * fb < 0.0 {
        let z: Bez = bz.iter().map(|&c| [c - level, 0.0, 0.0, 0.0]).collect();
        let mut l = 0.0;
        let mut h = 1.0;
        for _ in 0..200 {
            let m = 0.5 * (l + h);
            if (bez_eval(&z, m)?[0] < 0.0) == (fa < 0.0) {
                l = m;
            } else {
                h = m;
            }
            if h - l < 1e-17 {
                break;
            }
        }
        out.push(a + (b - a) * 0.5 * (l + h));
        return Ok(());
    }
    if fa == 0.0 || fb == 0.0 {
        return Ok(()); // ends at a break: no split inside
    }
    let z: Bez = bz.iter().map(|&c| [c, 0.0, 0.0, 0.0]).collect();
    let (l, r) = bez_split(&z, 0.5)?;
    let l: Vec<f64> = l.iter().map(|q| q[0]).collect();
    let r: Vec<f64> = r.iter().map(|q| q[0]).collect();
    roots(&l, level, a, 0.5 * (a + b), depth + 1, out)?;
    roots(&r, level, 0.5 * (a + b), b, depth + 1, out)
}

/// `sm_MxCompose`: `S(pc(t))`, the surface's homogeneous form along a polynomial curve
/// in its parameters, exact: `pc` is split where it crosses the patches' breaks, and on
/// each piece the patch's Bernstein basis in `(s(t), t(t))` is expanded by products.
pub fn compose(srf: &Srf, pc: &Crv) -> Result<Crv> {
    // the breaks of pc where it crosses a patch boundary
    let mut cuts = Vec::new();
    for (s, seg) in pc.seg.iter().enumerate() {
        let a = *g(&pc.brk, s)?;
        let b = *g(&pc.brk, s + 1)?;
        for k in 0..2 {
            let c: Vec<f64> = seg
                .iter()
                .map(|q| if k == 0 { q[0] } else { q[1] } / q[3])
                .collect();
            let lv = if k == 0 { &srf.ub } else { &srf.vb };
            for i in 1..lv.len().saturating_sub(1) {
                let mut rts = Vec::new();
                roots(&c, *g(lv, i)?, 0.0, 1.0, 0, &mut rts)?;
                for r in rts {
                    cuts.push(a + (b - a) * r);
                }
            }
        }
    }
    let pc = refine(pc, &cuts)?;
    let mut out = Crv {
        brk: pc.brk.clone(),
        seg: Vec::new(),
    };
    let power = |b: &[V4], k: usize| -> Result<Bez> {
        let mut r: Bez = vec![[1.0, 0.0, 0.0, 0.0]];
        for _ in 0..k {
            r = bez_times(b, &r)?;
        }
        Ok(r)
    };
    for q in &pc.seg {
        let m = bez_eval(q, 0.5)?;
        let x = m[0] / m[3];
        let y = m[1] / m[3];
        let mut i = 0;
        let mut j = 0;
        while i + 2 < srf.ub.len() && x > *g(&srf.ub, i + 1)? {
            i += 1;
        }
        while j + 2 < srf.vb.len() && y > *g(&srf.vb, j + 1)? {
            j += 1;
        }
        let u0 = *g(&srf.ub, i)?;
        let du = *g(&srf.ub, i + 1)? - *g(&srf.ub, i)?;
        let v0 = *g(&srf.vb, j)?;
        let dv = *g(&srf.vb, j + 1)? - *g(&srf.vb, j)?;
        // homogeneous local coordinates: s = X / W, t = Y / W, and W
        let mut xs = Bez::new();
        let mut ys = Bez::new();
        let mut xw = Bez::new();
        let mut yw = Bez::new();
        for c in q {
            xs.push([(c[0] - u0 * c[3]) / du, 0.0, 0.0, 0.0]);
            ys.push([(c[1] - v0 * c[3]) / dv, 0.0, 0.0, 0.0]);
            xw.push([c[3] - (c[0] - u0 * c[3]) / du, 0.0, 0.0, 0.0]);
            yw.push([c[3] - (c[1] - v0 * c[3]) / dv, 0.0, 0.0, 0.0]);
        }
        let mut bu = Vec::new();
        for a in 0..=srf.du {
            let mut t = bez_times(&power(&xs, a)?, &power(&xw, srf.du - a)?)?;
            for c in &mut t {
                *c = mul(*c, choose(srf.du, a));
            }
            bu.push(t);
        }
        let mut bv = Vec::new();
        for b in 0..=srf.dv {
            let mut t = bez_times(&power(&ys, b)?, &power(&yw, srf.dv - b)?)?;
            for c in &mut t {
                *c = mul(*c, choose(srf.dv, b));
            }
            bv.push(t);
        }
        let pts = srf.at(i, j)?;
        let mut r: Bez = Vec::new();
        for (a, bua) in bu.iter().enumerate() {
            for (b, bvb) in bv.iter().enumerate() {
                let basis = bez_times(bua, bvb)?;
                let cp = *g(pts, a * (srf.dv + 1) + b)?;
                let term: Bez = basis.iter().map(|c| mul(cp, c[0])).collect();
                if r.is_empty() {
                    r = term;
                } else {
                    for (k, slot) in r.iter_mut().enumerate() {
                        *slot = add(*slot, *g(&term, k)?);
                    }
                }
            }
        }
        out.seg.push(r);
    }
    Ok(out)
}

/// `sm_MxSrfDerivV`: `S`'s homogeneous derivative in v (each patch's control rows
/// differenced, degree `dv - 1`).
pub fn srf_deriv_v(srf: &Srf) -> Result<Srf> {
    let rdv = srf.dv.checked_sub(1).ok_or_else(err)?;
    let mut r = Srf {
        du: srf.du,
        dv: rdv,
        ub: srf.ub.clone(),
        vb: srf.vb.clone(),
        p: Vec::new(),
    };
    let nus = srf.ub.len().checked_sub(1).ok_or_else(err)?;
    let nvs = srf.vb.len().checked_sub(1).ok_or_else(err)?;
    r.p = vec![vec![V4::default(); (r.du + 1) * (r.dv + 1)]; nus * nvs];
    for i in 0..nus {
        for j in 0..nvs {
            let len = *g(&srf.vb, j + 1)? - *g(&srf.vb, j)?;
            let pts = srf.at(i, j)?;
            for a in 0..=srf.du {
                for b in 0..srf.dv {
                    *gm(gm(&mut r.p, i * nvs + j)?, a * (rdv + 1) + b)? = mul(
                        sub(
                            *g(pts, a * (srf.dv + 1) + b + 1)?,
                            *g(pts, a * (srf.dv + 1) + b)?,
                        ),
                        srf.dv as f64 / len,
                    );
                }
            }
        }
    }
    Ok(r)
}

/// `sm_MxSolve`: the least-norm solution of `A x = b` (rows of `A` given) by the eigen
/// decomposition of `A^T A` (Jacobi), and the residual `|A x - b|`.
fn solve(a: &[Vec<f64>], b: &[f64]) -> Result<(Vec<f64>, f64)> {
    let m = a.len();
    let n = a.first().map_or(0, Vec::len);
    let mut nn = vec![vec![0.0; n]; n];
    let mut v = vec![vec![0.0; n]; n];
    let mut r = vec![0.0; n];
    for i in 0..n {
        *gm(gm(&mut v, i)?, i)? = 1.0;
        for j in 0..n {
            for k in 0..m {
                let row = g(a, k)?;
                *gm(gm(&mut nn, i)?, j)? += *g(row, i)? * *g(row, j)?;
            }
        }
        for k in 0..m {
            *gm(&mut r, i)? += *g(g(a, k)?, i)? * *g(b, k)?;
        }
    }
    let at = |x: &Vec<Vec<f64>>, i: usize, j: usize| -> Result<f64> { Ok(*g(g(x, i)?, j)?) };
    for _sweep in 0..100 {
        let mut off = 0.0;
        for p in 0..n {
            for q in p + 1..n {
                off += at(&nn, p, q)? * at(&nn, p, q)?;
            }
        }
        if off < 1e-30 {
            break;
        }
        for p in 0..n {
            for q in p + 1..n {
                if at(&nn, p, q)?.abs() < 1e-300 {
                    continue;
                }
                let th = 0.5 * (2.0 * at(&nn, p, q)?).atan2(at(&nn, q, q)? - at(&nn, p, p)?);
                let c = th.cos();
                let s = th.sin();
                for k in 0..n {
                    let x = at(&nn, k, p)?;
                    let d = at(&nn, k, q)?;
                    let row = gm(&mut nn, k)?;
                    *gm(row, p)? = c * x - s * d;
                    *gm(row, q)? = s * x + c * d;
                }
                for k in 0..n {
                    let x = at(&nn, p, k)?;
                    let d = at(&nn, q, k)?;
                    *gm(gm(&mut nn, p)?, k)? = c * x - s * d;
                    *gm(gm(&mut nn, q)?, k)? = s * x + c * d;
                }
                for k in 0..n {
                    let x = at(&v, k, p)?;
                    let d = at(&v, k, q)?;
                    let row = gm(&mut v, k)?;
                    *gm(row, p)? = c * x - s * d;
                    *gm(row, q)? = s * x + c * d;
                }
            }
        }
    }
    let mut emax: f64 = 0.0;
    for i in 0..n {
        emax = max(emax, at(&nn, i, i)?.abs());
    }
    let mut x = vec![0.0; n];
    for i in 0..n {
        let nii = at(&nn, i, i)?;
        if nii <= 1e-14 * emax {
            continue;
        }
        let mut c = 0.0;
        for k in 0..n {
            c += at(&v, k, i)? * *g(&r, k)?;
        }
        c /= nii;
        for k in 0..n {
            *gm(&mut x, k)? += at(&v, k, i)? * c;
        }
    }
    let mut res = 0.0;
    for k in 0..m {
        let mut e = -*g(b, k)?;
        for i in 0..n {
            e += *g(g(a, k)?, i)? * *g(&x, i)?;
        }
        res += e * e;
    }
    Ok((x, res.sqrt()))
}

/// `SmMxSide`: a side of a corner face, its ribbon `g = alpha c + beta c' + gamma D`
/// (with `T` in `c'`'s place where the side has one).
#[derive(Debug, Clone, Default)]
pub struct Side {
    /// `c`: the side, on `[0, 1]`, end weights 1.
    pub c: Crv,
    /// `D`: its transversal direction field.
    pub dir: Crv,
    /// `bShared`: `D` is the field `M = A + k B` shared along X-V.
    pub shared: bool,
    /// `v`: alpha, beta, gamma at `u = 0, 1`.
    pub val: [[f64; 3]; 2],
    /// `d`: their derivatives.
    pub der: [[f64; 3]; 2],
    /// `Dv`: `D` at the ends.
    pub dir_val: [V4; 2],
    /// `Dd`: `D'` at the ends.
    pub dir_der: [V4; 2],
    /// `Dk`: `D'`'s part per unit of k at the ends.
    pub dir_k: [V4; 2],
    /// `T`: the cross field in `c'`'s place, where `c` is only C1.
    pub t: Crv,
    /// `bT`.
    pub has_t: bool,
}

/// `sm_MxSideEnds`: the side's `D` at its ends (an unshared side's own).
pub fn side_ends(s: &mut Side) -> Result<()> {
    let d = deriv(&s.dir)?;
    for (e, ((val, der), dk)) in s
        .dir_val
        .iter_mut()
        .zip(s.dir_der.iter_mut())
        .zip(s.dir_k.iter_mut())
        .enumerate()
    {
        *val = eval(&s.dir, e as f64)?;
        *der = eval(&d, e as f64)?;
        *dk = V4::default();
    }
    Ok(())
}

/// `sm_MxCrvDerivAt`: the `k`-th derivative of `c` at `t`.
pub fn crv_deriv_at(c: &Crv, t: f64, k: usize) -> Result<V4> {
    let mut r = c.clone();
    for _ in 0..k {
        r = deriv(&r)?;
    }
    eval(&r, t)
}

/// `sm_MxFirstOrder`: `alpha c + beta c' + gamma D` at the side's end `e` = `target`,
/// the first-order corner condition (residual within `1e-9 scale`).
pub fn first_order(s: &mut Side, e: usize, target: V4, scale: f64) -> Result<()> {
    let t = e as f64;
    let c = eval(&s.c, t)?;
    let c1 = if s.has_t {
        eval(&s.t, t)?
    } else {
        crv_deriv_at(&s.c, t, 1)?
    };
    let d = *g(&s.dir_val, e)?;
    let mut a = Vec::new();
    let mut b = Vec::new();
    for (((&ck, &c1k), &dk), &tk) in c.iter().zip(c1.iter()).zip(d.iter()).zip(target.iter()) {
        a.push(vec![ck, c1k, dk]);
        b.push(tk);
    }
    let (x, res) = solve(&a, &b)?;
    if res > 1e-9 * scale {
        // Mixed corner: a side's tangent leaves its neighbour's tangent plane at a corner
        return Err(err());
    }
    let slot = gm(&mut s.val, e)?;
    for (k, sl) in slot.iter_mut().take(3).enumerate() {
        *sl = *g(&x, k)?;
    }
    Ok(())
}

/// `SmMxTwistTerm`: one ribbon end in a twist equation: its side (an index into the
/// caller's sides), which end, and the sign it enters with.
#[derive(Debug, Clone, Copy)]
pub struct TwistTerm {
    /// `pS`.
    pub side: usize,
    /// `e`.
    pub e: usize,
    /// `sign`.
    pub sign: f64,
}

/// `sm_MxTwists`: the twists at corners, each equation's two terms' `sign * g'(e)`
/// summing to 0; the unknowns each term's `(alpha', beta', gamma')` and, with
/// `want_k`, the shared scalar k of the field `M = A + k B` (returned). Residual within
/// `1e-8 scale`.
pub fn twists(sides: &mut [Side], eqs: &[[TwistTerm; 2]], scale: f64, want_k: bool) -> Result<f64> {
    let terms: Vec<TwistTerm> = eqs.iter().flat_map(|q| q.iter().copied()).collect();
    let n = 3 * terms.len() + usize::from(want_k);
    let mut a = Vec::new();
    let mut b = Vec::new();
    for (qi, eq) in eqs.iter().enumerate() {
        let mut rows = vec![vec![0.0; n]; 4];
        let mut rhs = [0.0; 4];
        for (h, term) in eq.iter().enumerate() {
            let s = g(sides, term.side)?;
            let idx = 3 * (2 * qi + h);
            let t = term.e as f64;
            let c = eval(&s.c, t)?;
            let c1 = crv_deriv_at(&s.c, t, 1)?;
            // beta's field and its derivative: T where the side has one, else c' and c''
            let f = if s.has_t { eval(&s.t, t)? } else { c1 };
            let f1 = if s.has_t {
                crv_deriv_at(&s.t, t, 1)?
            } else {
                crv_deriv_at(&s.c, t, 2)?
            };
            let d = *g(&s.dir_val, term.e)?;
            let v = *g(&s.val, term.e)?;
            let (al, be, ga) = (v[0], v[1], v[2]);
            let known = add(
                add(mul(c1, al), mul(f1, be)),
                mul(*g(&s.dir_der, term.e)?, ga),
            );
            let dk = *g(&s.dir_k, term.e)?;
            for (k, rk) in rhs.iter_mut().enumerate() {
                let row = gm(&mut rows, k)?;
                *gm(row, idx)? += term.sign * *g(&c, k)?;
                *gm(row, idx + 1)? += term.sign * *g(&f, k)?;
                *gm(row, idx + 2)? += term.sign * *g(&d, k)?;
                if want_k {
                    *gm(row, n - 1)? += term.sign * ga * *g(&dk, k)?;
                }
                *rk -= term.sign * *g(&known, k)?;
            }
        }
        for (row, &rk) in rows.into_iter().zip(rhs.iter()) {
            a.push(row);
            b.push(rk);
        }
    }
    let (x, res) = solve(&a, &b)?;
    if res > 1e-8 * scale {
        // Mixed corner: the corner's twists are not compatible
        return Err(err());
    }
    for (i, term) in terms.iter().enumerate() {
        let s = gm(sides, term.side)?;
        let slot = gm(&mut s.der, term.e)?;
        for (k, sl) in slot.iter_mut().take(3).enumerate() {
            *sl = *g(&x, 3 * i + k)?;
        }
    }
    if want_k {
        Ok(*g(&x, n - 1)?)
    } else {
        Ok(0.0)
    }
}

/// `sm_MxRibbon`: the side's cross-derivative ribbon.
fn ribbon(s: &Side) -> Result<Crv> {
    let a = hermite(s.val[0][0], s.val[1][0], s.der[0][0], s.der[1][0]);
    let b = hermite(s.val[0][1], s.val[1][1], s.der[0][1], s.der[1][1]);
    let gm_ = hermite(s.val[0][2], s.val[1][2], s.der[0][2], s.der[1][2]);
    let mut r = times(&a, &s.c)?;
    let field = if s.has_t { s.t.clone() } else { deriv(&s.c)? };
    r = sum(&r, &times(&b, &field)?, 1.0, 1.0)?;
    r = sum(&r, &times(&gm_, &s.dir)?, 1.0, 1.0)?;
    Ok(r)
}

/// `sm_MxSrfMapU`: `f` applied to each column of control points in u (each a curve).
fn srf_map_u(srf: &Srf, f: &dyn Fn(&Crv) -> Result<Crv>) -> Result<Srf> {
    let nus = srf.ub.len().checked_sub(1).ok_or_else(err)?;
    let nvs = srf.vb.len().checked_sub(1).ok_or_else(err)?;
    let mut r = Srf {
        dv: srf.dv,
        vb: srf.vb.clone(),
        ..Srf::default()
    };
    for j in 0..nvs {
        for b in 0..=srf.dv {
            let mut c = Crv {
                brk: srf.ub.clone(),
                seg: Vec::new(),
            };
            for i in 0..nus {
                let pts = srf.at(i, j)?;
                let mut z = Bez::new();
                for a in 0..=srf.du {
                    z.push(*g(pts, a * (srf.dv + 1) + b)?);
                }
                c.seg.push(z);
            }
            let m = f(&c)?;
            if r.p.is_empty() {
                r.du = m.deg().ok_or_else(err)?;
                r.ub.clone_from(&m.brk);
                let nm = m.brk.len().checked_sub(1).ok_or_else(err)?;
                r.p = vec![vec![V4::default(); (r.du + 1) * (r.dv + 1)]; nm * nvs];
            }
            for (i, seg) in m.seg.iter().enumerate() {
                for a in 0..=r.du {
                    *gm(gm(&mut r.p, i * nvs + j)?, a * (r.dv + 1) + b)? = *g(seg, a)?;
                }
            }
        }
    }
    Ok(r)
}

/// `sm_MxSrfTranspose`.
pub fn srf_transpose(srf: &Srf) -> Result<Srf> {
    let nus = srf.ub.len().checked_sub(1).ok_or_else(err)?;
    let nvs = srf.vb.len().checked_sub(1).ok_or_else(err)?;
    let mut r = Srf {
        du: srf.dv,
        dv: srf.du,
        ub: srf.vb.clone(),
        vb: srf.ub.clone(),
        p: vec![vec![V4::default(); (srf.dv + 1) * (srf.du + 1)]; nus * nvs],
    };
    for i in 0..nus {
        for j in 0..nvs {
            let pts = srf.at(i, j)?;
            for a in 0..=srf.du {
                for b in 0..=srf.dv {
                    *gm(gm(&mut r.p, j * nus + i)?, b * (r.dv + 1) + a)? =
                        *g(pts, a * (srf.dv + 1) + b)?;
                }
            }
        }
    }
    Ok(r)
}

/// `sm_MxSrfMapV`.
fn srf_map_v(srf: &Srf, f: &dyn Fn(&Crv) -> Result<Crv>) -> Result<Srf> {
    srf_transpose(&srf_map_u(&srf_transpose(srf)?, f)?)
}

/// `sm_MxSrfCommon`: refined to the breaks and elevated to the degrees, direction by
/// direction.
fn srf_common(srf: &Srf, ub: &[f64], du: usize, vb: &[f64], dv: usize) -> Result<Srf> {
    let r = srf_map_u(srf, &|c| elevate(&refine(c, ub)?, du))?;
    srf_map_v(&r, &|c| elevate(&refine(c, vb)?, dv))
}

/// `sm_MxTensorU`: the sum over k of `curves[k](u)` times the cubic Bernstein row
/// `h[k](v)`.
fn tensor_u(mut curves: Vec<Crv>, h: &[[f64; 4]; 4]) -> Result<Srf> {
    for _pass in 0..2 {
        for k in 1..curves.len() {
            let (head, tail) = curves.split_at_mut(k);
            common(gm(head, 0)?, gm(tail, 0)?)?;
        }
    }
    let c0 = g(&curves, 0)?;
    let mut s = Srf {
        du: c0.deg().ok_or_else(err)?,
        dv: 3,
        ub: c0.brk.clone(),
        vb: vec![0.0, 1.0],
        p: Vec::new(),
    };
    for i in 0..c0.seg.len() {
        let mut pts = vec![V4::default(); (s.du + 1) * 4];
        for a in 0..=s.du {
            for b in 0..4 {
                for (k, ck) in curves.iter().enumerate() {
                    let slot = gm(&mut pts, a * 4 + b)?;
                    *slot = add(*slot, mul(*g(g(&ck.seg, i)?, a)?, *g(g(h, k)?, b)?));
                }
            }
        }
        s.p.push(pts);
    }
    Ok(s)
}

/// `sm_MxSrfSum`: `A + sb B`.
fn srf_sum(a: &Srf, b: &Srf, sb: f64) -> Result<Srf> {
    let ub = union(&a.ub, &b.ub);
    let vb = union(&a.vb, &b.vb);
    let du = a.du.max(b.du);
    let dv = a.dv.max(b.dv);
    let mut a = srf_common(a, &ub, du, &vb, dv)?;
    let b = srf_common(b, &ub, du, &vb, dv)?;
    for (p, pts) in a.p.iter_mut().enumerate() {
        let other = g(&b.p, p)?;
        for (k, q) in pts.iter_mut().enumerate() {
            *q = add(*q, mul(*g(other, k)?, sb));
        }
    }
    Ok(a)
}

/// `sm_MxCoons`: the bicubically blended Coons patch of four sides (`v = 0`, `v = 1`,
/// `u = 0`, `u = 1`) and their ribbons.
pub fn coons(c0: &Side, c1: &Side, d0: &Side, d1: &Side) -> Result<Srf> {
    // cubic Hermite blends in Bernstein form: H0, H1 (values), H~0, H~1 (derivatives)
    let h: [[f64; 4]; 4] = [
        [1.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 1.0],
        [0.0, 1.0 / 3.0, 0.0, 0.0],
        [0.0, 0.0, -1.0 / 3.0, 0.0],
    ];
    let p1 = tensor_u(
        vec![c0.c.clone(), c1.c.clone(), ribbon(c0)?, ribbon(c1)?],
        &h,
    )?;
    let p2 = srf_transpose(&tensor_u(
        vec![d0.c.clone(), d1.c.clone(), ribbon(d0)?, ribbon(d1)?],
        &h,
    )?)?;
    // the corners' data: positions, S_u, S_v, S_uv
    let mut sp = [[V4::default(); 2]; 2];
    let mut su = [[V4::default(); 2]; 2];
    let mut sv = [[V4::default(); 2]; 2];
    let mut suv = [[V4::default(); 2]; 2];
    let ac = [c0, c1];
    let ad = [d0, d1];
    for (i, (((spr, sur), svr), suvr)) in sp
        .iter_mut()
        .zip(su.iter_mut())
        .zip(sv.iter_mut())
        .zip(suv.iter_mut())
        .enumerate()
    {
        let di = *g(&ad, i)?;
        for (j, (((spv, suvl), svv), suvv)) in spr
            .iter_mut()
            .zip(sur.iter_mut())
            .zip(svr.iter_mut())
            .zip(suvr.iter_mut())
            .enumerate()
        {
            let cj = *g(&ac, j)?;
            *spv = eval(&cj.c, i as f64)?;
            *suvl = crv_deriv_at(&cj.c, i as f64, 1)?;
            *svv = crv_deriv_at(&di.c, j as f64, 1)?;
            *suvv = crv_deriv_at(&ribbon(cj)?, i as f64, 1)?;
        }
    }
    let hv = |i: usize, a: usize| -> Result<f64> { Ok(*g(g(&h, i)?, a)?) };
    let at = |m: &[[V4; 2]; 2], i: usize, j: usize| -> Result<V4> { Ok(*g(g(m, i)?, j)?) };
    let mut pts = vec![V4::default(); 16];
    for a in 0..4 {
        for b in 0..4 {
            for i in 0..2 {
                for j in 0..2 {
                    let slot = gm(&mut pts, a * 4 + b)?;
                    *slot = add(*slot, mul(at(&sp, i, j)?, hv(i, a)? * hv(j, b)?));
                    *slot = add(*slot, mul(at(&su, i, j)?, hv(2 + i, a)? * hv(j, b)?));
                    *slot = add(*slot, mul(at(&sv, i, j)?, hv(i, a)? * hv(2 + j, b)?));
                    *slot = add(*slot, mul(at(&suv, i, j)?, hv(2 + i, a)? * hv(2 + j, b)?));
                }
            }
        }
    }
    let p12 = Srf {
        du: 3,
        dv: 3,
        ub: vec![0.0, 1.0],
        vb: vec![0.0, 1.0],
        p: vec![pts],
    };
    srf_sum(&srf_sum(&p1, &p2, 1.0)?, &p12, -1.0)
}

/// `sm_MxCoonsBilinear`: the bilinearly blended Coons patch of four sides (G0).
pub fn coons_bilinear(c0: &Side, c1: &Side, d0: &Side, d1: &Side) -> Result<Srf> {
    // crU[k](u) times (1 - v), v
    let tensor = |mut u0: Crv, mut u1: Crv| -> Result<Srf> {
        common(&mut u0, &mut u1)?;
        let mut s = Srf {
            du: u0.deg().ok_or_else(err)?,
            dv: 1,
            ub: u0.brk.clone(),
            vb: vec![0.0, 1.0],
            p: Vec::new(),
        };
        for i in 0..u0.seg.len() {
            let mut pts = vec![V4::default(); (s.du + 1) * 2];
            for a in 0..=s.du {
                *gm(&mut pts, a * 2)? = *g(g(&u0.seg, i)?, a)?;
                *gm(&mut pts, a * 2 + 1)? = *g(g(&u1.seg, i)?, a)?;
            }
            s.p.push(pts);
        }
        Ok(s)
    };
    let p1 = tensor(c0.c.clone(), c1.c.clone())?;
    let p2 = srf_transpose(&tensor(d0.c.clone(), d1.c.clone())?)?;
    let p12 = Srf {
        du: 1,
        dv: 1,
        ub: vec![0.0, 1.0],
        vb: vec![0.0, 1.0],
        p: vec![vec![
            eval(&c0.c, 0.0)?,
            eval(&c1.c, 0.0)?,
            eval(&c0.c, 1.0)?,
            eval(&c1.c, 1.0)?,
        ]],
    };
    srf_sum(&srf_sum(&p1, &p2, 1.0)?, &p12, -1.0)
}

/// `sm_MxSrfToBSpline`: the patches as one rational B-spline surface (`SM_ERR` where a
/// weight is not positive).
pub fn srf_to_bspline(srf: &Srf) -> Result<BSplineSurface> {
    let nus = srf.ub.len().checked_sub(1).ok_or_else(err)?;
    let nvs = srf.vb.len().checked_sub(1).ok_or_else(err)?;
    if nus == 0 || nvs == 0 || srf.du == 0 || srf.dv == 0 {
        return Err(err());
    }
    let n_u = nus * srf.du + 1;
    let n_v = nvs * srf.dv + 1;
    let mut pts = Vec::new();
    let mut w = Vec::new();
    for ii in 0..n_u {
        for jj in 0..n_v {
            let i = (ii / srf.du).min(nus - 1);
            let j = (jj / srf.dv).min(nvs - 1);
            let q = *g(
                srf.at(i, j)?,
                (ii - i * srf.du) * (srf.dv + 1) + (jj - j * srf.dv),
            )?;
            if q[3].is_nan() || q[3] <= 0.0 {
                // Mixed corner: a face's weights are not all positive
                return Err(err());
            }
            pts.push(project(q));
            w.push(q[3]);
        }
    }
    let ends = |len: usize, d: usize| -> Vec<usize> {
        (0..len)
            .map(|i| if i == 0 || i + 1 == len { d + 1 } else { d })
            .collect()
    };
    let um = ends(srf.ub.len(), srf.du);
    let vm = ends(srf.vb.len(), srf.dv);
    BSplineSurface::create_canonical(&CanonicalSurfaceData {
        u_degree: srf.du,
        v_degree: srf.dv,
        control_points: &pts,
        form: BSplineSurfaceForm::Unspecified,
        u_multiplicities: &um,
        v_multiplicities: &vm,
        u_knots: &srf.ub,
        v_knots: &srf.vb,
        knot_type: KnotType::Unspecified,
        weights: Some(&w),
        uv_domain: None,
    })
}

/// `sm_MxPiece`: the piece of a curve between parameters `a` and `b`, on `[0, 1]` from
/// `a` to `b`.
pub fn piece(c: &Crv, a: f64, b: f64) -> Result<Crv> {
    let lo = min(a, b);
    let hi = max(a, b);
    let r = refine(c, &[lo, hi])?;
    let mut p = Crv::default();
    for (s, seg) in r.seg.iter().enumerate() {
        let b0 = *g(&r.brk, s)?;
        let b1 = *g(&r.brk, s + 1)?;
        if b0 >= lo - 1e-13 * (1.0 + lo.abs()) && b1 <= hi + 1e-13 * (1.0 + hi.abs()) {
            if p.brk.is_empty() {
                p.brk.push(b0);
            }
            p.seg.push(seg.clone());
            p.brk.push(b1);
        }
    }
    unit(&p, a > b)
}

/// `sm_MxToUV`: a uv curve (B-spline, rational where its weights differ) from a 2D
/// piecewise Bezier curve.
pub fn to_uv(c: &Crv) -> Result<BSplineCurve> {
    let p = c.deg().ok_or_else(err)?;
    let mut pts = Vec::new();
    let mut w: Vec<f64> = Vec::new();
    let mut rational = false;
    for (s, seg) in c.seg.iter().enumerate() {
        for i in usize::from(s != 0)..=p {
            let q = *g(seg, i)?;
            pts.push(Point3::new(q[0] / q[3], q[1] / q[3], 0.0));
            w.push(q[3]);
            if q[3] != first(&w)? {
                rational = true;
            }
        }
    }
    let n = c.brk.len();
    let mults: Vec<usize> = (0..n)
        .map(|i| if i == 0 || i + 1 == n { p + 1 } else { p })
        .collect();
    BSplineCurve::create_canonical(&CanonicalData {
        dim: 2,
        degree: p,
        control_points: &pts,
        form: BSplineCurveForm::Unspecified,
        knot_multiplicities: &mults,
        knots: &c.brk,
        knot_type: KnotType::Unspecified,
        weights: if rational { Some(&w) } else { None },
        trim: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(a: [f64; 3], b: [f64; 3]) -> Crv {
        Crv {
            brk: vec![0.0, 1.0],
            seg: vec![vec![
                point(Point3::new(a[0], a[1], a[2]), 1.0),
                point(Point3::new(b[0], b[1], b[2]), 1.0),
            ]],
        }
    }

    #[test]
    fn refine_and_elevate_keep_the_curve() {
        let c = elevate(&line([0.0, 0.0, 0.0], [2.0, 4.0, 6.0]), 3).unwrap();
        let r = refine(&c, &[0.25, 0.5]).unwrap();
        assert_eq!(r.brk, vec![0.0, 0.25, 0.5, 1.0]);
        for t in [0.1, 0.3, 0.6, 0.9] {
            let a = project(eval(&c, t).unwrap());
            let b = project(eval(&r, t).unwrap());
            assert!(a.distance(b) < 1e-14);
            assert!(a.distance(Point3::new(2.0 * t, 4.0 * t, 6.0 * t)) < 1e-14);
        }
    }

    #[test]
    fn unit_weights_makes_end_weights_one() {
        // a quarter circle's rational quadratic, weights 1, sqrt(1/2), 1 scaled by 2
        let w = 0.5f64.sqrt();
        let c = Crv {
            brk: vec![0.0, 1.0],
            seg: vec![vec![
                point(Point3::new(1.0, 0.0, 0.0), 2.0),
                point(Point3::new(1.0, 1.0, 0.0), 2.0 * w),
                point(Point3::new(0.0, 1.0, 0.0), 2.0),
            ]],
        };
        let u = unit_weights(&c).unwrap();
        assert!((eval(&u, 0.0).unwrap()[3] - 1.0).abs() < 1e-15);
        assert!((eval(&u, 1.0).unwrap()[3] - 1.0).abs() < 1e-15);
        for t in [0.2, 0.5, 0.7] {
            let p = project(eval(&u, t).unwrap());
            assert!((p.x * p.x + p.y * p.y - 1.0).abs() < 1e-14);
        }
    }

    #[test]
    fn solve_is_least_norm() {
        // x + y = 2 (one row): least-norm (1, 1)
        let (x, res) = solve(&[vec![1.0, 1.0]], &[2.0]).unwrap();
        assert!((x[0] - 1.0).abs() < 1e-14 && (x[1] - 1.0).abs() < 1e-14);
        assert!(res < 1e-14);
    }

    #[test]
    fn roots_finds_the_crossing() {
        let mut r = Vec::new();
        roots(&[0.0, 1.0], 0.25, 0.0, 1.0, 0, &mut r).unwrap();
        assert_eq!(r.len(), 1);
        assert!((r[0] - 0.25).abs() < 1e-15);
    }

    #[test]
    fn c1_keeps_a_c1_joint() {
        let c0 = elevate(&line([0.0, 0.0, 0.0], [2.0, 0.0, 0.0]), 2).unwrap();
        let mut c = refine(&c0, &[0.5]).unwrap();
        let before = c.clone();
        c1(&mut c, &mut []).unwrap();
        for (a, b) in c.seg.iter().flatten().zip(before.seg.iter().flatten()) {
            for k in 0..4 {
                assert!((a[k] - b[k]).abs() < 1e-14);
            }
        }
    }
}
