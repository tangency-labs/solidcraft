//! FFI wrappers for unit `surface.surface_p4`.
//!
//! Shim: `harness/cpp/shim/surface_surface_p4.cpp` (`sc_surface_surface_p4_eval`).
//! Upstream: `source/SMLib/src/SmSurface.cpp`, `source/SMLib/inc/SmSurface.h`.
//!
//! Every input starts with a surface block as in unit `surface.surface_p7`
//! (`super::surface_surface_p7`): `0 oob form n m p q r s cpts(x y z w) u v` (a B-spline
//! surface), `1 O[3] N[3]` (a plane) or `2 basis implicit` (a curve-bounded surface
//! without boundary curves). A curve block is `dim n p m cpts(x y z w) knots`. A
//! domain is `u0 v0 u1 v1`; `pts` is `count` then `count` parameter vectors of the
//! equation set's size; `vecs` is `n` then `n` vectors; `tgt` is `has value`. Every
//! output starts with the status (`0` = `SM_SUCCESS`, else the `SmStatus`); the rest:
//!
//! | [`Op`]          | upstream                                    | parameters after the surface                     | outputs |
//! |-----------------|---------------------------------------------|--------------------------------------------------|---------|
//! | `NormalEval`    | `SmSurfaceEval::Normal`                     | `u v`                                            | `N` |
//! | `SectionFn`     | `smsurf_EvaluateNormalSection`              | `u v dir[3]`                                     | `T k` |
//! | `SectionEval`   | `SmSurfaceEval::EvalNormalSection`          | `u v dir[3]`                                     | `N T k` |
//! | `OnNormal`      | `smsurf_IsPointOnNormal`                    | `has dom u v P[3] has d[3] tol`                  | `fu fv good reason drop dev` |
//! | `PassSp`        | `sm_PassSPConvergenceTest`                  | `u v P[3] tol`                                   | `pass` |
//! | `Seam`          | `SmSurface::CreateSeamCurve`                | `param known`                                    | `created [t0 t1 (st P[3])*5]` |
//! | `IsOnSeam`      | `SmSurface::IsOnSeam`                       | `u v has tol`                                    | `on dir n uv*n` |
//! | `Gps`           | `SmSurface::GlobalPointSolve`               | `dom sop P[3] tol tgt req`                       | `n (value u v)*n` |
//! | `CsIntersect`   | `SmCSIntersectENFO::Evaluate`               | `curve pts[3]`                                   | per point `st [found F[3] J[9]]` |
//! | `CsNormalize`   | `SmCSNormalizeENFO::Evaluate`               | `curve sop vecs pts[3 or 4]`                     | per point `st [found F J]` |
//! | `SspIntersect`  | `SmSSPIntersectENFO::Evaluate`              | `surface steepest has O[3] N[3] pts[4]`          | per point `st [found F[4] J[16]]` |
//! | `SsiNormalize`  | `SmSSINormalizeENFO::Evaluate`              | `surface N[3] d pts[4]`                          | per point `st [found F[4] J[16]]` |
//! | `SsNormalize`   | `SmSSNormalizeENFO::Evaluate`               | `surface sop vecs pts[4 or 5]`                   | per point `st [found F J]` |
//! | `Classify`      | `sm_ClassifyCurveSurfaceXSectPoint`         | `dom curve a b u v t`                            | `found u v t type` |
//! | `StepInv`       | `SmBSplineSurface::STEPInversion` (planes)  | `dom P[3] tol has guess[2]`                      | `st [u v loc]` |
//!
//! The equation sets are verbatim copies of the file-local upstream classes; the
//! stateful `CsIntersect` keeps one object across its points. [`eval`] accepts only
//! what upstream handles without undefined behaviour or runaway work (as unit
//! `surface.surface_p7`): well-formed clamped B-splines, planes with a non-zero normal,
//! parameters within one domain length of the surface's domain, signed directed
//! solves with a vector, B-spline surfaces for the seam, at most 32 points per call.

use crate::{call_with_buffer, FfiError};

use super::surface_surface_p7::{
    bad, curve, domain_in, sop, surface, surface_kind, target, uv, vectors, Domain, Reader,
    SO_AT_DISTANCE, SO_INTERSECT, SO_MAXIMIZE, SO_MINIMIZE, SO_NORMALIZE, SO_SIGNED_MIN,
};

/// Most points per equation-set call.
pub const MAX_POINTS: usize = 32;

/// Shim operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum Op {
    NormalEval,
    SectionFn,
    SectionEval,
    OnNormal,
    PassSp,
    Seam,
    IsOnSeam,
    Gps,
    CsIntersect,
    CsNormalize,
    SspIntersect,
    SsiNormalize,
    SsNormalize,
    Classify,
    StepInv,
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
        pub fn sc_surface_surface_p4_eval(
            op: i32,
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

/// `count` then `count` parameter vectors of `dim` finite values each; `bounds`, when
/// given, are the `[lo, hi]` each coordinate must lie in.
fn points(rd: &mut Reader<'_>, dim: usize, bounds: &[(f64, f64)]) -> Result<(), FfiError> {
    let count = rd.index(MAX_POINTS)?;
    if count == 0 {
        return Err(bad("no points"));
    }
    for _ in 0..count {
        for k in 0..dim {
            let x = rd.finite()?;
            if let Some(&(lo, hi)) = bounds.get(k) {
                if x < lo || x > hi {
                    return Err(bad("parameter too far outside its domain"));
                }
            }
        }
    }
    Ok(())
}

/// The bounds of the `(u, v)` parameters of a surface (one domain length around it).
fn uv_bounds(dom: Domain) -> Vec<(f64, f64)> {
    match dom {
        Some([u0, v0, u1, v1]) => vec![(u0 - (u1 - u0), u1 + (u1 - u0)), (v0 - (v1 - v0), v1 + (v1 - v0))],
        None => vec![(-1e4, 1e4), (-1e4, 1e4)],
    }
}

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
        Op::NormalEval => uv(&mut rd, dom)?,
        Op::SectionFn | Op::SectionEval => {
            uv(&mut rd, dom)?;
            rd.vec3()?;
        }
        Op::OnNormal => {
            let has = rd.flag()?;
            if has {
                domain_in(&mut rd, dom)?;
            } else {
                rd.finites(4)?;
            }
            uv(&mut rd, dom)?;
            rd.vec3()?;
            rd.flag()?;
            rd.vec3()?;
            rd.positive("tolerance")?;
        }
        Op::PassSp => {
            uv(&mut rd, dom)?;
            rd.vec3()?;
            rd.positive("tolerance")?;
        }
        Op::Seam => {
            bspline_only(dom)?;
            rd.index(1)?;
            rd.flag()?;
        }
        Op::IsOnSeam => {
            uv(&mut rd, dom)?;
            rd.flag()?;
            rd.positive("tolerance")?;
        }
        Op::Gps => {
            domain_in(&mut rd, dom)?;
            let s = sop(
                &mut rd,
                &[SO_INTERSECT, SO_MINIMIZE, SO_MAXIMIZE, SO_NORMALIZE, SO_AT_DISTANCE],
            )?;
            rd.vec3()?;
            rd.positive("tolerance")?;
            if !target(&mut rd)? && s == SO_AT_DISTANCE {
                return Err(bad("AT_DISTANCE needs a target"));
            }
            rd.index(1)?;
        }
        Op::CsIntersect => {
            let (a, b) = curve(&mut rd)?;
            let mut bounds = vec![(a - (b - a), b + (b - a))];
            bounds.extend(uv_bounds(dom));
            points(&mut rd, 3, &bounds)?;
        }
        Op::CsNormalize => {
            let (a, b) = curve(&mut rd)?;
            let s = sop(&mut rd, &[SO_MINIMIZE, SO_MAXIMIZE, SO_NORMALIZE, SO_SIGNED_MIN])?;
            let n = vectors(&mut rd)?;
            let directed = s == SO_SIGNED_MIN;
            if directed && n == 0 {
                return Err(bad("signed directed solve needs a vector"));
            }
            let mut bounds = vec![(a - (b - a), b + (b - a))];
            bounds.extend(uv_bounds(dom));
            points(&mut rd, if directed { 4 } else { 3 }, &bounds)?;
        }
        Op::SspIntersect => {
            let d2 = surface(&mut rd)?;
            rd.flag()?;
            rd.flag()?;
            rd.vec3()?;
            rd.nonzero3()?;
            let mut bounds = uv_bounds(dom);
            bounds.extend(uv_bounds(d2));
            points(&mut rd, 4, &bounds)?;
        }
        Op::SsiNormalize => {
            let d2 = surface(&mut rd)?;
            rd.nonzero3()?;
            rd.finite()?;
            let mut bounds = uv_bounds(dom);
            bounds.extend(uv_bounds(d2));
            points(&mut rd, 4, &bounds)?;
        }
        Op::SsNormalize => {
            let d2 = surface(&mut rd)?;
            let s = sop(&mut rd, &[SO_MINIMIZE, SO_MAXIMIZE, SO_NORMALIZE, SO_SIGNED_MIN])?;
            let n = vectors(&mut rd)?;
            let directed = s == SO_SIGNED_MIN;
            if directed && n == 0 {
                return Err(bad("signed directed solve needs a vector"));
            }
            let mut bounds = uv_bounds(dom);
            bounds.extend(uv_bounds(d2));
            points(&mut rd, if directed { 5 } else { 4 }, &bounds)?;
        }
        Op::StepInv => {
            if dom.is_some() {
                return Err(bad("this op needs a plane"));
            }
            domain_in(&mut rd, dom)?;
            rd.vec3()?;
            rd.positive("tolerance")?;
            rd.flag()?;
            uv(&mut rd, dom)?;
        }
        Op::Classify => {
            domain_in(&mut rd, dom)?;
            let (a, b) = curve(&mut rd)?;
            let x = rd.finite()?;
            let y = rd.finite()?;
            if x >= y || x < a - (b - a) || y > b + (b - a) {
                return Err(bad("interval outside the curve's range"));
            }
            uv(&mut rd, dom)?;
            let t = rd.finite()?;
            if t < a - (b - a) || t > b + (b - a) {
                return Err(bad("curve parameter too far outside"));
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
            sys::sc_surface_surface_p4_eval(
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
