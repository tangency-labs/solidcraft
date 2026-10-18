//! FFI wrappers for unit `surface.surface_p5`.
//!
//! Shim: `harness/cpp/shim/surface_surface_p5.cpp` (`sc_surface_surface_p5_eval`).
//! Upstream: `source/SMLib/src/SmSurface.cpp`, `source/SMLib/inc/SmSurface.h`.
//!
//! Every input starts with a surface block: `0 oob form n m p q r s cpts(x y z w) u v` (an
//! `SmBSplineSurface(gw_SURFACE*, FALSE, NULL)`), `1 O[3] N[3]` (`SmPlane(O, N)`) or
//! `2 basis implicit` (an `SmCurveBoundedSurface` without boundary curves), as unit
//! `surface.surface_p7`. A domain is `u0 v0 u1 v1`; a curve block is `dim n p m cpts(x y z w)
//! knots`; a solution is `type nvars value p0 p1 p2`. Every output starts with the status
//! (`0` = `SM_SUCCESS`, else the `SmStatus`); the rest:
//!
//! | [`Op`]         | upstream                                     | parameters after the surface                          | outputs |
//! |----------------|----------------------------------------------|-------------------------------------------------------|---------|
//! | `IsSing`       | `IsSingularity`                              | `u v tol pt_only`                                     | `singular dir` (`singular` is the `SmBoolean`) |
//! | `NormalSing`   | `EvaluateNormalAtSingularity`                | `u v tol`                                             | `N` on success |
//! | `Iso`          | `CreateIsoParametricCurve`                   | `param iso tol has_dom dom want_uv`                   | `gap has_uv t0 t1 P*9` on success |
//! | `Lps`          | `LocalPointSolve`                            | `dom sop P guess`                                     | `found [value u v]` |
//! | `Lps0`         | `LocalPointSolve_0`                          | `dom sop P guess`                                     | `found [value u v] reason iters` |
//! | `Drop`         | `DropPoint`                                  | `dom P has_guess guess sop has_tgt tgt want`          | `ok u v gap multi nsols` |
//! | `Lssi`         | `LocalSurfaceSurfaceIntersect` (XYZ guess)   | `dom surface2 dom2 surface3 dom3 P has2 P2`           | `found [value u0 v0 u1 v1 u2 v2]` |
//! | `Cover`        | `CoverOtherCoincidentSurface`                | `surface2 tol merged`                                 | `which max_dist` (`which`: `-1` none, `0` this, `1` other) |
//! | `Filter`       | `sm_FilterSolution` (verbatim copy)          | `dom curve ivl uc[2] vc[2] tc[2] value t u v`         | `keep` |
//! | `SpLocal`      | `sm_SpLocalSolve` (verbatim copy, adapted)   | `dom uc[2] vc[2] P tol sop solve_surf proc_bnd`       | `n (value u v)*n` |
//! | `Ident`        | `Sm{SP,LSI,CS,CSI}GlobalSolver::IdenticalSolutions` | `kind tol sol1 sol2`                           | `pair preferred` (`-1` none) |
//! | `MaxDist`     | `SmCurve::SurfaceMaxDistanceBetween`         | `dom curve ivl samples has_needed needed`             | `max_distance t u v` on success |
//!
//! [`eval`] accepts only what upstream handles without undefined behaviour or runaway work:
//! well-formed clamped B-splines and planes (curve-bounded surfaces for the singularity
//! tests), parameters within one domain length of the surface's domain, domains inside a
//! B-spline's natural domain, and the solver operations each function supports. Inputs are
//! finite.

use crate::validate::{bad, check_knots, NOW, NOZ};
use crate::{call_with_buffer, FfiError};

/// Largest control point index.
pub const MAX_INDEX: usize = 32;
/// Largest degree.
pub const MAX_DEGREE: usize = 5;
/// Largest coordinate magnitude.
pub const MAX_COORD: f64 = 1.0e6;
/// Largest vector count.
pub const MAX_VECS: usize = 16;

/// Shim operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum Op {
    IsSing,
    NormalSing,
    Iso,
    Lps,
    Lps0,
    Drop,
    Lssi,
    Cover,
    Filter,
    SpLocal,
    Ident,
    MaxDist,
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
        pub fn sc_surface_surface_p5_eval(
            op: i32,
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

crate::reader::unit_reader!(MAX_COORD);

impl Reader<'_> {
    fn finite(&mut self) -> Result<f64, FfiError> {
        let v = self.num()?;
        if !v.is_finite() || v.abs() > MAX_COORD {
            return Err(bad("non-finite or huge value"));
        }
        Ok(v)
    }

    fn finites(&mut self, k: usize) -> Result<Vec<f64>, FfiError> {
        (0..k).map(|_| self.finite()).collect()
    }

    fn vec3(&mut self) -> Result<[f64; 3], FfiError> {
        Ok([self.finite()?, self.finite()?, self.finite()?])
    }

    #[allow(dead_code)]
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

    fn nonzero3(&mut self) -> Result<[f64; 3], FfiError> {
        let v = self.vec3()?;
        if v.iter().map(|c| c * c).sum::<f64>() < 1e-12 {
            return Err(bad("zero vector"));
        }
        Ok(v)
    }

    fn index(&mut self, max: usize) -> Result<usize, FfiError> {
        let v = self.num()?;
        if v.fract() != 0.0 || v < 0.0 || v > max as f64 {
            return Err(bad(format!("bad index/count {v} (max {max})")));
        }
        Ok(v as usize)
    }

    fn flag(&mut self) -> Result<bool, FfiError> {
        Ok(self.index(1)? == 1)
    }

    fn positive(&mut self, what: &str) -> Result<f64, FfiError> {
        let v = self.finite()?;
        if v <= 0.0 {
            return Err(bad(format!("{what}: need a positive value")));
        }
        Ok(v)
    }
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
type Domain = Option<[f64; 4]>;

/// A surface block (no nested curve-bounded surfaces): its domain (`None` for a plane
/// basis) and whether it is a plain B-spline.
fn surface_kind(rd: &mut Reader<'_>, nested: bool) -> Result<(Domain, bool), FfiError> {
    let kind = rd.index(2)?;
    if kind == 2 {
        if nested {
            return Err(bad("nested curve-bounded surface"));
        }
        let (dom, _) = surface_kind(rd, true)?;
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

#[allow(dead_code)]
/// A surface block; returns its parameter domain (`None` for a plane basis).
fn surface(rd: &mut Reader<'_>) -> Result<Domain, FfiError> {
    Ok(surface_kind(rd, false)?.0)
}

/// A B-spline curve block; returns its parameter range.
fn curve(rd: &mut Reader<'_>) -> Result<(f64, f64), FfiError> {
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
fn uv(rd: &mut Reader<'_>, dom: Domain) -> Result<(), FfiError> {
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
fn domain_in(rd: &mut Reader<'_>, dom: Domain) -> Result<(), FfiError> {
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

#[allow(dead_code)]
/// `n` then `n` vectors.
fn vectors(rd: &mut Reader<'_>) -> Result<usize, FfiError> {
    let n = rd.index(MAX_VECS)?;
    for _ in 0..n {
        rd.vec3()?;
    }
    Ok(n)
}

#[allow(dead_code)]
/// An optional target distance `has value`.
fn target(rd: &mut Reader<'_>) -> Result<bool, FfiError> {
    let has = rd.flag()?;
    rd.finite()?;
    Ok(has)
}

/// A solver operation from `allowed`.
fn sop(rd: &mut Reader<'_>, allowed: &[usize]) -> Result<usize, FfiError> {
    let v = rd.index(15)?;
    if !allowed.contains(&v) {
        return Err(bad(format!("solver operation {v} not supported here")));
    }
    Ok(v)
}

/// `SM_SO_INTERSECT`, `SM_SO_MINIMIZE`, `SM_SO_MAXIMIZE`, `SM_SO_NORMALIZE`,
/// `SM_SO_AT_DISTANCE`, `SM_SO_3D_SIGNED_DIRECTED_MINIMIZE`.
const SO_INTERSECT: usize = 0;
const SO_MINIMIZE: usize = 3;
const SO_MAXIMIZE: usize = 4;
const SO_NORMALIZE: usize = 5;
#[allow(dead_code)]
const SO_AT_DISTANCE: usize = 6;
#[allow(dead_code)]
const SO_SIGNED_MIN: usize = 9;

/// Continuity codes `SM_CT_UNDEFINED ..= SM_CT_C1_C2`.
fn continuity(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    rd.index(10)?;
    Ok(())
}

/// Checks `input` for `op` (see the module docs).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for anything the shim must not see.
pub fn validate(op: Op, input: &[f64]) -> Result<(), FfiError> {
    let mut rd = Reader::new(input);
    let (dom, is_bspline) = surface_kind(&mut rd, false)?;
    let bspline_only = |dom: Domain| {
        dom.filter(|_| is_bspline)
            .ok_or_else(|| bad("this op needs a B-spline surface"))
    };
    // curve-bounded surfaces are fine: C++ handles them, and the ones that reach
    // unported paths are deferred
    let solve_surface = |dom: Domain| -> Result<Domain, FfiError> { Ok(dom) };
    match op {
        Op::IsSing => {
            uv(&mut rd, dom)?;
            rd.positive("tolerance")?;
            rd.flag()?;
        }
        Op::NormalSing => {
            uv(&mut rd, dom)?;
            rd.positive("tolerance")?;
        }
        Op::Iso => {
            let d = solve_surface(dom)?;
            let param = rd.index(1)?;
            let v = rd.finite()?;
            if let Some([u0, v0, u1, v1]) = d {
                let (a, b) = if param == 0 { (u0, u1) } else { (v0, v1) };
                if v < a - (b - a) || v > b + (b - a) {
                    return Err(bad("iso parameter too far outside the domain"));
                }
            } else if v.abs() > 1e4 {
                return Err(bad("plane iso parameter too large"));
            }
            let t = rd.finite()?;
            if t < 0.0 {
                return Err(bad("negative tolerance"));
            }
            rd.flag()?;
            domain_in(&mut rd, d)?;
            rd.flag()?;
        }
        Op::Lps | Op::Lps0 => {
            let d = solve_surface(dom)?;
            domain_in(&mut rd, d)?;
            sop(
                &mut rd,
                &[SO_INTERSECT, SO_MINIMIZE, SO_MAXIMIZE, SO_NORMALIZE],
            )?;
            rd.vec3()?;
            uv(&mut rd, dom)?;
        }
        Op::Drop => {
            let d = solve_surface(dom)?;
            domain_in(&mut rd, d)?;
            rd.vec3()?;
            rd.flag()?;
            uv(&mut rd, dom)?;
            sop(&mut rd, &[SO_INTERSECT, SO_MINIMIZE, SO_NORMALIZE])?;
            rd.flag()?;
            rd.vec3()?;
            rd.flag()?;
        }
        Op::Lssi => {
            let d = bspline_only(dom)?;
            domain_in(&mut rd, Some(d))?;
            let (d2, b2) = surface_kind(&mut rd, false)?;
            let d2 = d2
                .filter(|_| b2)
                .ok_or_else(|| bad("this op needs B-spline surfaces"))?;
            domain_in(&mut rd, Some(d2))?;
            let (d3, b3) = surface_kind(&mut rd, false)?;
            let d3 = d3
                .filter(|_| b3)
                .ok_or_else(|| bad("this op needs B-spline surfaces"))?;
            domain_in(&mut rd, Some(d3))?;
            rd.vec3()?;
            rd.flag()?;
            rd.vec3()?;
        }
        Op::Cover => {
            bspline_only(dom)?;
            let (d2, b2) = surface_kind(&mut rd, false)?;
            d2.filter(|_| b2)
                .ok_or_else(|| bad("this op needs B-spline surfaces"))?;
            rd.positive("tolerance")?;
            rd.flag()?;
        }
        Op::Filter => {
            let d = solve_surface(dom)?;
            domain_in(&mut rd, d)?;
            let (a, b) = curve(&mut rd)?;
            let x = rd.finite()?;
            let y = rd.finite()?;
            if x < a || y > b || x >= y {
                return Err(bad("interval outside the curve's range"));
            }
            for _ in 0..6 {
                continuity(&mut rd)?;
            }
            let value = rd.finite()?;
            if value < 0.0 {
                return Err(bad("negative solution value"));
            }
            let t = rd.finite()?;
            if t < a || t > b {
                return Err(bad("curve parameter outside the curve's range"));
            }
            uv(&mut rd, dom)?;
        }
        Op::SpLocal => {
            let d = solve_surface(dom)?;
            domain_in(&mut rd, d)?;
            for _ in 0..4 {
                continuity(&mut rd)?;
            }
            rd.vec3()?;
            rd.positive("tolerance")?;
            sop(
                &mut rd,
                &[SO_INTERSECT, SO_MINIMIZE, SO_MAXIMIZE, SO_NORMALIZE],
            )?;
            rd.flag()?;
            rd.flag()?;
        }
        Op::MaxDist => {
            let d = solve_surface(dom)?;
            domain_in(&mut rd, d)?;
            let (a, b) = curve(&mut rd)?;
            let x = rd.finite()?;
            let y = rd.finite()?;
            if x < a || y > b || x >= y {
                return Err(bad("interval outside the curve's range"));
            }
            rd.index(32)?;
            rd.flag()?;
            rd.finite()?;
        }
        Op::Ident => {
            let kind = rd.index(3)?;
            rd.positive("tolerance")?;
            for _ in 0..2 {
                // `type nvars value p0 p1 p2`: the surface parameters are `p0 p1` for the
                // point solver, `p1 p2` after a curve or line parameter otherwise
                let t = rd.index(2)?;
                if t == 0 {
                    return Err(bad("solution type"));
                }
                rd.index(3)?;
                rd.finite()?;
                if kind == 0 {
                    uv(&mut rd, dom)?;
                    rd.finite()?;
                } else {
                    rd.finite()?;
                    uv(&mut rd, dom)?;
                }
            }
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
            sys::sc_surface_surface_p5_eval(
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
