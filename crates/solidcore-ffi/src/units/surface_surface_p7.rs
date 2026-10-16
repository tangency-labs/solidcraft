//! FFI wrappers for unit `surface.surface_p7`.
//!
//! Shim: `harness/cpp/shim/surface_surface_p7.cpp` (`sc_surface_surface_p7_eval`).
//! Upstream: `source/SMLib/src/SmSurface.cpp`, `source/SMLib/inc/SmSurface.h`.
//!
//! Every input starts with a surface block: `0 oob form n m p q r s cpts(x y z w) u v` (an
//! `SmBSplineSurface(gw_SURFACE*, FALSE, NULL)`, as unit `surface.surface_p2`), `1 O[3]
//! N[3]` (`SmPlane(O, N)`) or `2 basis implicit` (an `SmCurveBoundedSurface` without
//! boundary curves over a B-spline or plane basis block). A curve block is `dim n p m cpts(x y z w) knots` (an
//! `SmBSplineCurve(gw_CURVE*, dim, FALSE)`). A domain is `u0 v0 u1 v1`; `ul vl` are the
//! `bUFromLeft` / `bVFromLeft` flags; `vecs` is `n` then `n` vectors; `tgt` is `has value`
//! (an optional target distance). `Gcs` / `Gss` list their first 16 solutions (`nv` at
//! most 4; `end` only for a range). Every output starts with the status (`0` =
//! `SM_SUCCESS`, else the `SmStatus`); the rest:
//!
//! | [`Op`]         | upstream                                  | parameters after the surface        | outputs |
//! |----------------|-------------------------------------------|-------------------------------------|---------|
//! | `Eval1`        | `Evaluate1stDerivatives`                  | `u v ul vl`                         | `P Du Dv` on success |
//! | `Eval2`        | `Evaluate2ndDerivatives`                  | `u v ul vl`                         | `P Du Dv Duv Duu Dvv` |
//! | `DirDeriv`     | `EvaluateDirectionalDerivs`               | `u v du dv h ul vl`                 | `D[0..=h]` |
//! | `Normal`       | `EvaluateNormal` (`base`: `SmSurface::`)  | `u v ul vl base`                    | `N` |
//! | `IsClosed`     | `IsClosed` (`base`: `SmSurface::`)        | `dom param has_tol tol want base`   | `closed cont` (`-1` = not asked) |
//! | `IsPlanar`     | `IsPlanar` (`base`: `SmSurface::`)        | `tol base`                          | `planar P N dev` |
//! | `ApproxLen`    | `ApproxDerivativeLengths`                 | `dom`                               | `lu lv` |
//! | `Higher`       | `ComputeHigherOrderDerivs`                | `u v g1u g1v want3 want4`           | `G2 [G3] [G4]` |
//! | `Inward`       | `ComputeInwardVector`                     | `u v`                               | `V` |
//! | `DropPts`      | `DropPointsToTangentPlane`                | `u v ul vl vecs`                    | `uv*n` |
//! | `DropVecs`     | `DropVectors`                             | `u v ul vl vecs`                    | `uv*n` |
//! | `Degen`        | `FindDegenParamForDirection`              | `u v dir[3] from_pole sdir tol`     | `u v` (always: `rUV`) |
//! | `Flat`         | `GetFlatCorners`                          | `angle want`                        | `flags [P*4]` |
//! | `Brep`         | `GetBrep() != NULL`                       |                                     | `has` |
//! | `Gps0`         | `GlobalPointSolve_0`                      | `dom sop P tol tgt req`             | `n` |
//! | `Gcs`          | `GlobalCurveSolve`                        | `dom curve a b sop tol vecs tgt req`| `n [type value nv start[nv] (end[nv])]*` |
//! | `Gss`          | `GlobalSurfaceSolve`                      | `dom surface dom2 sop tol vecs tgt req` | `n [type value nv start[nv] (end[nv])]*` |
//! | `Lcs`          | `LocalCurveSolve`                         | `dom curve a b sop tol vecs gu gv gt` | `found [value t u v]` |
//! | `Proj`         | `CreateParallelProjectionCurves`          | `dom curve V[3] tgt`                | `n3d` |
//! | `LoadGuesses`  | `sm_LoadGuesses` (verbatim copy)          | `dom P sop`                         | `u v` |
//! | `PassSc`       | `sm_PassSCConvergenceTest`                | `u v curve t`                       | `pass` |
//! | `PassSs`       | `sm_PassSSConvergenceTest`                | `u v surface u2 v2`                 | `pass` |
//! | `GoodDrop`     | `sm_GoodDropResult`                       | `dom P gu gv value su sv`           | `good` |
//! | `CheckSeam`    | `sm_CheckSeamDrop`                        | `u v dom surface u2 v2 dom2 loc`    | `u v` on success |
//!
//! [`eval`] accepts only what upstream handles without undefined behaviour or runaway
//! work: well-formed clamped B-splines (as unit `surface.surface_p2`), planes with a
//! non-zero normal, parameters within one domain length of the surface's domain,
//! domains inside a B-spline's natural domain, solver operations each function supports
//! (the signed directed ones with a vector, `AT_DISTANCE` with a target), B-spline
//! surfaces for the global and local solves and the projection, `h <= 4`, location codes
//! `0..=5`. Inputs are finite, except that the projection vector may have a NaN
//! component (`ZERO_VEC_ER` rejects it first).

use crate::{call_with_buffer, FfiError};

/// Largest control point index.
pub const MAX_INDEX: usize = 32;
/// Largest degree.
pub const MAX_DEGREE: usize = 5;
/// Largest coordinate magnitude.
pub const MAX_COORD: f64 = 1.0e6;
/// Largest vector count.
pub const MAX_VECS: usize = 16;

pub(crate) const NOW: f64 = -f64::MAX;
pub(crate) const NOZ: f64 = f64::MAX;

/// Shim operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum Op {
    Eval1,
    Eval2,
    DirDeriv,
    Normal,
    IsClosed,
    IsPlanar,
    ApproxLen,
    Higher,
    Inward,
    DropPts,
    DropVecs,
    Degen,
    Flat,
    Brep,
    Gps0,
    Gcs,
    Gss,
    Lcs,
    Proj,
    LoadGuesses,
    PassSc,
    PassSs,
    GoodDrop,
    CheckSeam,
}

impl Op {
    /// The shim's op code.
    #[must_use]
    pub fn code(self) -> i32 {
        self as i32
    }
}

mod sys {
    extern "C" {
        pub fn sc_surface_surface_p7_eval(
            op: i32,
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

pub(crate) fn bad(msg: impl Into<String>) -> FfiError {
    FfiError::InvalidArgument(msg.into())
}

/// Sequential reader over the input.
pub(crate) struct Reader<'a> {
    pub(crate) input: &'a [f64],
    pub(crate) at: usize,
}

impl Reader<'_> {
    pub(crate) fn num(&mut self) -> Result<f64, FfiError> {
        let v = self
            .input
            .get(self.at)
            .copied()
            .ok_or_else(|| bad("input too short"))?;
        self.at += 1;
        Ok(v)
    }

    pub(crate) fn finite(&mut self) -> Result<f64, FfiError> {
        let v = self.num()?;
        if !v.is_finite() || v.abs() > MAX_COORD {
            return Err(bad("non-finite or huge value"));
        }
        Ok(v)
    }

    pub(crate) fn finites(&mut self, k: usize) -> Result<Vec<f64>, FfiError> {
        (0..k).map(|_| self.finite()).collect()
    }

    pub(crate) fn vec3(&mut self) -> Result<[f64; 3], FfiError> {
        Ok([self.finite()?, self.finite()?, self.finite()?])
    }

    /// A vector that is finite, or has a NaN component (upstream's `ZERO_VEC_ER`
    /// then fails before anything reads it).
    fn finite_or_nan3(&mut self) -> Result<[f64; 3], FfiError> {
        let v = [self.num()?, self.num()?, self.num()?];
        if v.iter().any(|c| c.is_nan()) {
            return Ok(v);
        }
        if v.iter().any(|c| !c.is_finite() || c.abs() > MAX_COORD) {
            return Err(bad("non-finite or huge value"));
        }
        Ok(v)
    }

    pub(crate) fn nonzero3(&mut self) -> Result<[f64; 3], FfiError> {
        let v = self.vec3()?;
        if v.iter().map(|c| c * c).sum::<f64>() < 1e-12 {
            return Err(bad("zero vector"));
        }
        Ok(v)
    }

    pub(crate) fn index(&mut self, max: usize) -> Result<usize, FfiError> {
        let v = self.num()?;
        if v.fract() != 0.0 || v < 0.0 || v > max as f64 {
            return Err(bad(format!("bad index/count {v} (max {max})")));
        }
        Ok(v as usize)
    }

    pub(crate) fn flag(&mut self) -> Result<bool, FfiError> {
        Ok(self.index(1)? == 1)
    }

    pub(crate) fn positive(&mut self, what: &str) -> Result<f64, FfiError> {
        let v = self.finite()?;
        if v <= 0.0 {
            return Err(bad(format!("{what}: need a positive value")));
        }
        Ok(v)
    }

    pub(crate) fn done(&self) -> Result<(), FfiError> {
        if self.at != self.input.len() {
            return Err(bad("trailing input"));
        }
        Ok(())
    }
}

/// Checks a clamped knot vector of degree `p`.
fn check_knots(knots: &[f64], p: usize, what: &str) -> Result<(f64, f64), FfiError> {
    let m = knots
        .len()
        .checked_sub(1)
        .ok_or_else(|| bad(format!("{what}: no knots")))?;
    if knots.windows(2).any(|w| w.first() > w.last()) {
        return Err(bad(format!("{what}: knots not non-decreasing")));
    }
    let (Some(&a), Some(&b)) = (knots.first(), knots.last()) else {
        return Err(bad(format!("{what}: empty knots")));
    };
    let short = || bad(format!("{what}: knots too short"));
    if m < 2 * p + 1 {
        return Err(short());
    }
    let head = knots.get(..=p).ok_or_else(short)?;
    let tail = knots.get(m - p..).ok_or_else(short)?;
    let inner = knots.get(p + 1..m - p).ok_or_else(short)?;
    if a >= b || head.iter().any(|&k| k != a) || tail.iter().any(|&k| k != b) {
        return Err(bad(format!("{what}: knots not clamped")));
    }
    if b - a < 1e-6 * a.abs().max(b.abs()).max(1.0) {
        return Err(bad(format!("{what}: parameter range too small")));
    }
    if inner.iter().any(|&k| k == a || k == b) {
        return Err(bad(format!("{what}: end multiplicity above degree + 1")));
    }
    let mut run = 1;
    for w in inner.windows(2) {
        run = if w.first() == w.last() { run + 1 } else { 1 };
        if run > p {
            return Err(bad(format!(
                "{what}: interior multiplicity above the degree"
            )));
        }
    }
    Ok((a, b))
}

/// Checks `k` control points sharing their rational marker (`z = NOZ` allowed for
/// curves when `planar`).
fn check_points(rd: &mut Reader<'_>, k: usize, planar: bool, what: &str) -> Result<(), FfiError> {
    let mut w0 = None;
    for _ in 0..k {
        let x = rd.num()?;
        let y = rd.num()?;
        let z = rd.num()?;
        let w = rd.num()?;
        if z == NOZ && !planar {
            return Err(bad(format!("{what}: 2D data")));
        }
        let rational = w != NOW;
        if *w0.get_or_insert(rational) != rational {
            return Err(bad(format!("{what}: mixed rationality")));
        }
        if rational && !(1e-3..=1e3).contains(&w) {
            return Err(bad(format!("{what}: weight out of range")));
        }
        let z = if z == NOZ { 0.0 } else { z };
        if [x, y, z]
            .iter()
            .any(|c| !c.is_finite() || c.abs() > MAX_COORD)
        {
            return Err(bad(format!("{what}: point out of range")));
        }
    }
    Ok(())
}

/// The parameter domain `[u0, v0, u1, v1]` of a surface block, `None` for a plane.
pub(crate) type Domain = Option<[f64; 4]>;

/// A surface block (no nested curve-bounded surfaces): its domain (`None` for a plane
/// basis) and whether it is a plain B-spline.
///
/// Kind 3 (a surface of revolution) is accepted only when `revolution` is set: only the
/// `surface_p3` shim builds one; the `surface_p4`/`surface_p7` shims have no such branch.
pub(crate) fn surface_kind(
    rd: &mut Reader<'_>,
    nested: bool,
    revolution: bool,
) -> Result<(Domain, bool), FfiError> {
    let kind = rd.index(if revolution { 3 } else { 2 })?;
    if kind == 3 {
        // a surface of revolution of the line a-b about the axis p, axis (domain: its own)
        for _ in 0..3 {
            rd.vec3()?;
        }
        rd.nonzero3()?;
        rd.flag()?;
        rd.flag()?;
        // an `SmBSplineSurface` subclass: the `SmBSplineSurface` members apply
        return Ok((None, true));
    }
    if kind == 2 {
        if nested {
            return Err(bad("nested curve-bounded surface"));
        }
        let (dom, _) = surface_kind(rd, true, revolution)?;
        rd.flag()?;
        return Ok((dom, false));
    }
    if kind == 1 {
        rd.vec3()?;
        rd.nonzero3()?;
        return Ok((None, false));
    }
    rd.flag()?;
    rd.index(11)?;
    let n = rd.index(MAX_INDEX)?;
    let m = rd.index(MAX_INDEX)?;
    let p = rd.index(MAX_DEGREE)?;
    let q = rd.index(MAX_DEGREE)?;
    let r = rd.index(MAX_INDEX + MAX_DEGREE + 1)?;
    let s = rd.index(MAX_INDEX + MAX_DEGREE + 1)?;
    if p == 0 || p > n || q == 0 || q > m || r != n + p + 1 || s != m + q + 1 {
        return Err(bad(
            "surface: need 1 <= p <= n, 1 <= q <= m, r = n+p+1, s = m+q+1",
        ));
    }
    check_points(rd, (n + 1) * (m + 1), false, "surface")?;
    let u = rd.finites(r + 1)?;
    let v = rd.finites(s + 1)?;
    let (u0, u1) = check_knots(&u, p, "surface u")?;
    let (v0, v1) = check_knots(&v, q, "surface v")?;
    Ok((Some([u0, v0, u1, v1]), true))
}

/// A surface block; returns its parameter domain (`None` for a plane basis).
pub(crate) fn surface(rd: &mut Reader<'_>) -> Result<Domain, FfiError> {
    Ok(surface_kind(rd, false, false)?.0)
}

/// As [`surface`], also accepting kind 3 (a surface of revolution; `surface_p3` only).
pub(crate) fn surface_rev(rd: &mut Reader<'_>) -> Result<Domain, FfiError> {
    Ok(surface_kind(rd, false, true)?.0)
}

/// A B-spline curve block; returns its parameter range.
pub(crate) fn curve(rd: &mut Reader<'_>) -> Result<(f64, f64), FfiError> {
    let dim = rd.index(3)?;
    if dim < 2 {
        return Err(bad("curve: dimension must be 2 or 3"));
    }
    let n = rd.index(MAX_INDEX)?;
    let p = rd.index(MAX_DEGREE)?;
    let m = rd.index(MAX_INDEX + MAX_DEGREE + 1)?;
    if p == 0 || p > n || m != n + p + 1 {
        return Err(bad("curve: need 1 <= p <= n and m = n + p + 1"));
    }
    check_points(rd, n + 1, dim == 2, "curve")?;
    let knots = rd.finites(m + 1)?;
    check_knots(&knots, p, "curve")
}

/// A `uv` point within one domain length of `dom` (anywhere bounded for a plane).
pub(crate) fn uv(rd: &mut Reader<'_>, dom: Domain) -> Result<(), FfiError> {
    let u = rd.finite()?;
    let v = rd.finite()?;
    if let Some([u0, v0, u1, v1]) = dom {
        let (du, dv) = (u1 - u0, v1 - v0);
        if u < u0 - du || u > u1 + du || v < v0 - dv || v > v1 + dv {
            return Err(bad("uv too far outside the domain"));
        }
    } else if u.abs() > 1e4 || v.abs() > 1e4 {
        return Err(bad("plane uv too large"));
    }
    Ok(())
}

/// A non-degenerate domain, inside `dom` for a B-spline.
pub(crate) fn domain_in(rd: &mut Reader<'_>, dom: Domain) -> Result<(), FfiError> {
    let d = rd.finites(4)?;
    let (Some(&a), Some(&b), Some(&c), Some(&e)) = (d.first(), d.get(1), d.get(2), d.get(3)) else {
        return Err(bad("domain too short"));
    };
    if a >= c || b >= e {
        return Err(bad("domain not ordered"));
    }
    if let Some([u0, v0, u1, v1]) = dom {
        if a < u0 || c > u1 || b < v0 || e > v1 {
            return Err(bad("domain outside the surface's"));
        }
    } else if a.abs() > 1e4 || b.abs() > 1e4 || c.abs() > 1e4 || e.abs() > 1e4 {
        return Err(bad("plane domain too large"));
    }
    Ok(())
}

/// `n` then `n` vectors.
pub(crate) fn vectors(rd: &mut Reader<'_>) -> Result<usize, FfiError> {
    let n = rd.index(MAX_VECS)?;
    for _ in 0..n {
        rd.vec3()?;
    }
    Ok(n)
}

/// An optional target distance `has value`.
pub(crate) fn target(rd: &mut Reader<'_>) -> Result<bool, FfiError> {
    let has = rd.flag()?;
    rd.finite()?;
    Ok(has)
}

/// A solver operation from `allowed`.
pub(crate) fn sop(rd: &mut Reader<'_>, allowed: &[usize]) -> Result<usize, FfiError> {
    let v = rd.index(15)?;
    if !allowed.contains(&v) {
        return Err(bad(format!("solver operation {v} not supported here")));
    }
    Ok(v)
}

/// `SM_SO_INTERSECT`, `SM_SO_MINIMIZE`, `SM_SO_MAXIMIZE`, `SM_SO_NORMALIZE`,
/// `SM_SO_AT_DISTANCE`, `SM_SO_3D_SIGNED_DIRECTED_MINIMIZE`.
pub(crate) const SO_INTERSECT: usize = 0;
pub(crate) const SO_MINIMIZE: usize = 3;
pub(crate) const SO_MAXIMIZE: usize = 4;
pub(crate) const SO_NORMALIZE: usize = 5;
pub(crate) const SO_AT_DISTANCE: usize = 6;
pub(crate) const SO_SIGNED_MIN: usize = 9;

/// Checks `input` for `op` (see the module docs).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for anything the shim must not see.
pub fn validate(op: Op, input: &[f64]) -> Result<(), FfiError> {
    let mut rd = Reader { input, at: 0 };
    let (dom, is_bspline) = surface_kind(&mut rd, false, false)?;
    let bspline_only = |dom: Domain| {
        dom.filter(|_| is_bspline)
            .ok_or_else(|| bad("this op needs a B-spline surface"))
    };
    match op {
        Op::Eval1 | Op::Eval2 => {
            uv(&mut rd, dom)?;
            rd.flag()?;
            rd.flag()?;
        }
        Op::DirDeriv => {
            uv(&mut rd, dom)?;
            rd.finite()?;
            rd.finite()?;
            rd.index(4)?;
            rd.flag()?;
            rd.flag()?;
        }
        Op::Normal => {
            uv(&mut rd, dom)?;
            rd.flag()?;
            rd.flag()?;
            rd.flag()?;
        }
        Op::IsClosed => {
            domain_in(&mut rd, dom)?;
            rd.index(3)?;
            rd.flag()?;
            rd.finite()?;
            rd.flag()?;
            rd.flag()?;
        }
        Op::IsPlanar => {
            rd.finite()?;
            rd.flag()?;
        }
        Op::ApproxLen => domain_in(&mut rd, dom)?,
        Op::Higher => {
            uv(&mut rd, dom)?;
            rd.finite()?;
            rd.finite()?;
            rd.flag()?;
            rd.flag()?;
        }
        Op::Inward => uv(&mut rd, dom)?,
        Op::DropPts | Op::DropVecs => {
            uv(&mut rd, dom)?;
            rd.flag()?;
            rd.flag()?;
            vectors(&mut rd)?;
        }
        Op::Degen => {
            uv(&mut rd, dom)?;
            rd.vec3()?;
            rd.flag()?;
            rd.index(1)?;
            rd.positive("tolerance")?;
        }
        Op::Flat => {
            let a = rd.positive("angle")?;
            if a > 90.0 {
                return Err(bad("angle above 90 degrees"));
            }
            rd.flag()?;
        }
        Op::Brep => {}
        Op::Gps0 => {
            let d = bspline_only(dom)?;
            domain_in(&mut rd, Some(d))?;
            let s = sop(
                &mut rd,
                &[
                    SO_INTERSECT,
                    SO_MINIMIZE,
                    SO_MAXIMIZE,
                    SO_NORMALIZE,
                    SO_AT_DISTANCE,
                ],
            )?;
            rd.vec3()?;
            rd.positive("tolerance")?;
            if !target(&mut rd)? && s == SO_AT_DISTANCE {
                return Err(bad("AT_DISTANCE needs a target"));
            }
            rd.index(1)?;
        }
        Op::Gcs | Op::Lcs => {
            let d = bspline_only(dom)?;
            domain_in(&mut rd, Some(d))?;
            let (a, b) = curve(&mut rd)?;
            let x = rd.finite()?;
            let y = rd.finite()?;
            if x < a || y > b || x >= y {
                return Err(bad("interval outside the curve's range"));
            }
            let s = sop(
                &mut rd,
                &[SO_MINIMIZE, SO_MAXIMIZE, SO_NORMALIZE, SO_SIGNED_MIN],
            )?;
            rd.positive("tolerance")?;
            if vectors(&mut rd)? == 0 && s == SO_SIGNED_MIN {
                return Err(bad("signed directed solve needs a vector"));
            }
            if op == Op::Gcs {
                target(&mut rd)?;
                rd.index(1)?;
            } else {
                uv(&mut rd, Some(d))?;
                rd.finite()?;
            }
        }
        Op::Gss => {
            let d = bspline_only(dom)?;
            domain_in(&mut rd, Some(d))?;
            let (d2, b2) = surface_kind(&mut rd, false, false)?;
            let d2 = d2
                .filter(|_| b2)
                .ok_or_else(|| bad("this op needs a B-spline surface"))?;
            domain_in(&mut rd, Some(d2))?;
            let s = sop(
                &mut rd,
                &[
                    SO_INTERSECT,
                    SO_MINIMIZE,
                    SO_MAXIMIZE,
                    SO_NORMALIZE,
                    SO_SIGNED_MIN,
                ],
            )?;
            rd.positive("tolerance")?;
            // (the signed directed leaf solves read m_cpOptVectors[0])
            if vectors(&mut rd)? == 0 && s == SO_SIGNED_MIN {
                return Err(bad("signed directed solve needs a vector"));
            }
            target(&mut rd)?;
            // intersect only for the node pairs (SM_SR_NODES, upstream's SM_ASSERT)
            let req = rd.index(3)?;
            if (s == SO_INTERSECT) != (req == 3) {
                return Err(bad(
                    "intersect needs SM_SR_NODES, and only intersect may ask for it",
                ));
            }
        }
        Op::Proj => {
            let d = bspline_only(dom)?;
            domain_in(&mut rd, Some(d))?;
            curve(&mut rd)?;
            rd.finite_or_nan3()?;
            if target(&mut rd)? && rd.input.get(rd.at - 1).is_some_and(|&t| t <= 0.0) {
                return Err(bad("projection distance must be positive"));
            }
        }
        Op::LoadGuesses => {
            domain_in(&mut rd, dom)?;
            rd.vec3()?;
            sop(
                &mut rd,
                &[SO_INTERSECT, SO_MINIMIZE, SO_MAXIMIZE, SO_NORMALIZE],
            )?;
        }
        Op::PassSc => {
            uv(&mut rd, dom)?;
            let (a, b) = curve(&mut rd)?;
            let t = rd.finite()?;
            if t < a - (b - a) || t > b + (b - a) {
                return Err(bad("curve parameter too far outside"));
            }
        }
        Op::PassSs => {
            uv(&mut rd, dom)?;
            let d2 = surface(&mut rd)?;
            uv(&mut rd, d2)?;
        }
        Op::GoodDrop => {
            domain_in(&mut rd, dom)?;
            rd.vec3()?;
            uv(&mut rd, dom)?;
            rd.finite()?;
            uv(&mut rd, dom)?;
        }
        Op::CheckSeam => {
            uv(&mut rd, dom)?;
            domain_in(&mut rd, dom)?;
            let d2 = surface(&mut rd)?;
            uv(&mut rd, d2)?;
            domain_in(&mut rd, d2)?;
            rd.index(5)?;
        }
    }
    rd.done()
}

/// Runs `op` on `input` (see the module docs) and returns the output record.
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for a malformed input; [`FfiError::Status`] for a shim
/// failure.
pub fn eval(op: Op, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    validate(op, input)?;
    call_with_buffer(256, |buf, len| {
        // SAFETY: input is valid for input.len() reads and holds exactly what the shim
        // reads for `op` (validated above); buf is valid for buf.len() writes and the
        // shim honours the capacity; len is valid.
        unsafe {
            sys::sc_surface_surface_p7_eval(
                op.code(),
                input.as_ptr(),
                input.len() as i64,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The p7 (and p4) shims have no surface-of-revolution branch, so kind 3 must be
    /// rejected by their validators.
    #[test]
    fn kind_3_rejected() {
        let probe = [
            3.0, 1.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.5, 0.5,
            0.0, 0.0,
        ];
        assert!(validate(Op::Eval1, &probe).is_err());
        assert!(validate(Op::Normal, &probe).is_err());
        assert!(super::super::surface_surface_p4::validate(
            super::super::surface_surface_p4::Op::NormalEval,
            &probe
        )
        .is_err());
    }
}
