//! FFI wrappers for unit `surface.surface_p1`.
//!
//! Shim: `harness/cpp/shim/surface_surface_p1.cpp` (`sc_surface_surface_p1_eval`).
//! Upstream: `source/SMLib/src/SmSurface.cpp`, `source/SMLib/src/SmSTEPSurface.cpp`.
//!
//! Notation as in [`super::surface_surface_cache_p3`]: `S` a surface `kind ..` (`0 B`:
//! B-spline `oob form N`; `1 o x y sx sy D`: bounded `SmPlane`; `2 B`:
//! `SmCurveBoundedSurface` over the B-spline), `W S` the surface wrapped in an
//! `SmSTEPSurface` owning it when `W = 1`, `V` a vector, `uv` a point `u v`, `D` a domain
//! `umin vmin umax vmax`, `C` a curve `dim n p m cpts(x y z w) knots`. Flags are `0` / `1`;
//! a parameter direction flag is `1` for `SM_SP_V`.
//!
//! | [`Op`]       | upstream                                               | parameters            |
//! |--------------|--------------------------------------------------------|-----------------------|
//! | `Eval`       | `Evaluate1stDerivatives`, `Evaluate2ndDerivatives`, `EvaluateSTEP`, `EvaluateSTEPPoint`, `EvaluateSimple` | `W S uv ul vl` |
//! | `Periodic`   | `IsPeriodic`                                           | `W S D v`             |
//! | `DegenPoint` | `IsDegeneratePoint` (base body, virtual)               | `W S tol hasD D`      |
//! | `DegenCurve` | `IsDegenerateCurve` (base body, virtual)               | `W S ang hasD D tol`  |
//! | `Flat`       | `IsFlatCorner`                                         | `W S uv ang`          |
//! | `OnBoundary` | `IsOnBoundary` (uv up to a domain side outside for planes and clamping B-splines) | `W S D uv hasTol tol` |
//! | `InDomain`   | `IsInDomain`                                           | `W S D uv hasTol tol` |
//! | `Misc`       | `operator==` (virtual, base), `IsAnalytic`, `GetFace`, `GetType` | `W S W S`   |
//! | `Snap`       | `SnapToKnots`                                          | `W S uv tol`          |
//! | `NSect`      | `smsurf_EvaluateNormalSection`                         | `9 V, dir V`          |
//! | `ParamDir`   | `ComputeParamDirection`                                | `W S uv V`            |
//! | `DirDerivs`  | `EvaluateDirectionalDerivs`                            | `W S uv dir h ul vl`  |
//! | `Convert`    | `ConvertDomainFromSTEPToNURBS`, `ConvertDomainFromNURBSToSTEP` | `W S D`       |
//! | `Size`       | `ApproximateSize`                                      | `W S hasD D`          |
//! | `FindIvl`    | `FindUVCurveIvlInDomain`                               | `C lo hi param D`     |
//! | `Sing`       | `SmFindSingularParamWithGivenTangentDirEFO::Evaluate`  | `W S uv V v tol s`    |
//! | `Seam`       | `sm_CrossedSeamOrPole`                                 | `W S uv uv D`         |
//! | `IsoLen`     | `SmIsoCurveLengthIFEO::Evaluate`                       | `W S u t`             |
//! | `Step`       | `SmSTEPSurface` `GetNaturalUVDomain`, `GetDomainWithContinuity`, `GetKnots` | `S v hasIvl lo hi` |
//! | `Cache`      | `CacheMakeOrValidate` (new cache)                      | `W S trim`            |
//! | `BBox`       | `CalculateBoundingBox`                                 | `W S D normal pseudo expand polar` |
//! | `DomCont`    | `GetDomainWithContinuity`                              | `W S cont uv V`       |
//! | `Sil`        | `Transform` (translation, optional), `CreateSilhouetteCurves` | `S D persp eye(V) hasTol tol hasAng ang hasMove move(V)` |
//!
//! The output is the op status (`0` = `SM_SUCCESS`, else the `SmStatus`) and the op's
//! outputs (see the shim). [`eval`] accepts only what upstream handles without undefined
//! behaviour: the surfaces of [`super::surface_surface_cache_p3`], points and domains
//! inside the surface domain wherever the surface is evaluated, directional derivative
//! orders the shim's array holds, valid enumerators, finite values; `FindIvl` a 2D curve,
//! an interval inside its domain and a trim domain of positive size; `Sil` a B-spline or a
//! plane (not wrapped) over a domain of positive size inside its domain, a non-zero view
//! vector for a parallel projection (upstream asserts it), and tolerances that keep the trace
//! bounded: an approximation tolerance of at least [`MIN_SIL_TOL`] times the surface size
//! (else the march takes millions of steps), an angle tolerance in
//! [[`MIN_SIL_ANG`], `pi`].

use super::surface_surface_cache_p1::{bad, curve, Reader, Srf};
use super::surface_surface_cache_p3::{sized_surface, Kind};
use crate::{call_with_buffer, FfiError};

/// The upstream routine run by the shim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// The evaluation wrappers.
    Eval,
    /// `IsPeriodic`.
    Periodic,
    /// `IsDegeneratePoint`.
    DegenPoint,
    /// `IsDegenerateCurve`.
    DegenCurve,
    /// `IsFlatCorner`.
    Flat,
    /// `IsOnBoundary`.
    OnBoundary,
    /// `IsInDomain`.
    InDomain,
    /// `operator==`, `IsAnalytic`, `GetFace`, `GetType`.
    Misc,
    /// `SnapToKnots`.
    Snap,
    /// `smsurf_EvaluateNormalSection`.
    NSect,
    /// `ComputeParamDirection`.
    ParamDir,
    /// `EvaluateDirectionalDerivs`.
    DirDerivs,
    /// The domain conversions.
    Convert,
    /// `ApproximateSize`.
    Size,
    /// `FindUVCurveIvlInDomain`.
    FindIvl,
    /// `SmFindSingularParamWithGivenTangentDirEFO::Evaluate`.
    Sing,
    /// `sm_CrossedSeamOrPole`.
    Seam,
    /// `SmIsoCurveLengthIFEO::Evaluate`.
    IsoLen,
    /// The `SmSTEPSurface` members.
    Step,
    /// `CacheMakeOrValidate`.
    Cache,
    /// `CalculateBoundingBox`.
    BBox,
    /// `GetDomainWithContinuity`.
    DomCont,
    /// `CreateSilhouetteCurves` (after an optional translation).
    Sil,
}

impl Op {
    /// The shim's op code.
    #[must_use]
    pub fn code(self) -> i32 {
        match self {
            Op::Eval => 0,
            Op::Periodic => 1,
            Op::DegenPoint => 2,
            Op::DegenCurve => 3,
            Op::Flat => 4,
            Op::OnBoundary => 5,
            Op::InDomain => 6,
            Op::Misc => 7,
            Op::Snap => 8,
            Op::NSect => 9,
            Op::ParamDir => 10,
            Op::DirDerivs => 11,
            Op::Convert => 12,
            Op::Size => 13,
            Op::FindIvl => 14,
            Op::Sing => 15,
            Op::Seam => 16,
            Op::IsoLen => 17,
            Op::Step => 18,
            Op::Cache => 19,
            Op::BBox => 20,
            Op::DomCont => 21,
            Op::Sil => 22,
        }
    }
}

mod sys {
    extern "C" {
        pub fn sc_surface_surface_p1_eval(
            op: i32,
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

/// Highest directional derivative order (the shim's array holds 8 vectors).
pub const MAX_DIR_DERIV: usize = 7;

/// Smallest `Sil` approximation tolerance, relative to the surface size.
pub const MIN_SIL_TOL: f64 = 1e-6;

/// Smallest `Sil` angle tolerance (radians).
pub const MIN_SIL_ANG: f64 = 1e-3;

/// Highest `SmContinuityType`.
const MAX_CONTINUITY: usize = 13;

/// A `W S` block: its kind and domain.
fn wrapped(rd: &mut Reader<'_>) -> Result<(Kind, Srf), FfiError> {
    rd.flag()?;
    let (kind, srf, _) = sized_surface(rd)?;
    Ok((kind, srf))
}

/// A `W S` block: whether it is wrapped, its kind, its domain, and whether its B-spline
/// evaluates out of bounds (`kind oob form N`).
fn wrapped_full(rd: &mut Reader<'_>) -> Result<(bool, Kind, Srf, bool), FfiError> {
    let wrap = rd.flag()?;
    let at = rd.at;
    let (kind, srf, _) = sized_surface(rd)?;
    let oob = kind != Kind::Plane && rd.input.get(at + 1).copied() == Some(1.0);
    Ok((wrap, kind, srf, oob))
}

/// A point `uv` within a side of the surface domain from it (upstream evaluates there:
/// B-splines without out-of-bounds evaluation clamp, planes are analytic).
fn uv_near(rd: &mut Reader<'_>, srf: &Srf) -> Result<[f64; 2], FfiError> {
    let u = rd.finite()?;
    let v = rd.finite()?;
    let d = srf.dom;
    let (w, h) = (d[2] - d[0], d[3] - d[1]);
    if u < d[0] - w || u > d[2] + w || v < d[1] - h || v > d[3] + h {
        return Err(bad("uv too far outside the surface domain"));
    }
    Ok([u, v])
}

/// A point `uv` inside the surface domain.
fn uv_in(rd: &mut Reader<'_>, srf: &Srf) -> Result<[f64; 2], FfiError> {
    let u = rd.finite()?;
    let v = rd.finite()?;
    let d = srf.dom;
    if u < d[0] || u > d[2] || v < d[1] || v > d[3] {
        return Err(bad("uv outside the surface domain"));
    }
    Ok([u, v])
}

/// A `hasD D` pair, `D` inside the surface domain.
fn opt_domain(rd: &mut Reader<'_>, srf: &Srf) -> Result<(), FfiError> {
    if rd.flag()? {
        rd.domain_in(srf, false)?;
    } else {
        rd.domain()?;
    }
    Ok(())
}

/// A `hasTol tol` pair: a positive tolerance when given.
fn opt_tol(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    let has = rd.flag()?;
    let tol = rd.finite()?;
    if has && tol <= 0.0 {
        return Err(bad("tolerance must be positive"));
    }
    Ok(())
}

/// A curve `C` of dimension 2; returns its parameter domain.
fn uv_curve(rd: &mut Reader<'_>) -> Result<(f64, f64), FfiError> {
    let at = rd.at;
    curve(rd)?;
    let get = |i: usize| rd.input.get(i).copied().unwrap_or(0.0);
    if get(at) != 2.0 {
        return Err(bad("find interval: the curve must be 2D"));
    }
    let (n, m) = (get(at + 1) as usize, get(at + 3) as usize);
    let knots = at + 4 + 4 * (n + 1);
    Ok((get(knots), get(knots + m)))
}

fn validate_op(op: Op, rd: &mut Reader<'_>) -> Result<(), FfiError> {
    match op {
        Op::Eval => {
            let (_, srf) = wrapped(rd)?;
            uv_in(rd, &srf)?;
            rd.flag()?;
            rd.flag()?;
        }
        Op::Periodic => {
            let (_, srf) = wrapped(rd)?;
            rd.domain_in(&srf, false)?;
            rd.flag()?;
        }
        Op::DegenPoint => {
            let (_, srf) = wrapped(rd)?;
            rd.finite()?;
            opt_domain(rd, &srf)?;
        }
        Op::DegenCurve => {
            let (_, srf) = wrapped(rd)?;
            rd.finite()?;
            opt_domain(rd, &srf)?;
            rd.finite()?;
        }
        Op::Flat => {
            let (_, srf) = wrapped(rd)?;
            uv_in(rd, &srf)?;
            rd.finite()?;
        }
        Op::OnBoundary | Op::InDomain => {
            let (_, kind, srf, oob) = wrapped_full(rd)?;
            rd.domain()?;
            if kind == Kind::Plane || (kind == Kind::BSpline && !oob) {
                uv_near(rd, &srf)?;
            } else {
                uv_in(rd, &srf)?;
            }
            opt_tol(rd)?;
        }
        Op::Misc => {
            wrapped(rd)?;
            wrapped(rd)?;
        }
        Op::Snap => {
            let (_, srf) = wrapped(rd)?;
            uv_in(rd, &srf)?;
            if rd.finite()? < 0.0 {
                return Err(bad("snap: negative tolerance"));
            }
        }
        Op::NSect => {
            rd.finites(30)?;
        }
        Op::ParamDir => {
            let (_, srf) = wrapped(rd)?;
            uv_in(rd, &srf)?;
            rd.vec3()?;
        }
        Op::DirDerivs => {
            let (_, srf) = wrapped(rd)?;
            uv_in(rd, &srf)?;
            rd.finites(2)?;
            rd.index(MAX_DIR_DERIV)?;
            rd.flag()?;
            rd.flag()?;
        }
        Op::Convert => {
            let (_, srf) = wrapped(rd)?;
            rd.domain_in(&srf, false)?;
        }
        Op::Size => {
            let (_, srf) = wrapped(rd)?;
            opt_domain(rd, &srf)?;
        }
        Op::FindIvl => {
            let (c0, c1) = uv_curve(rd)?;
            let lo = rd.finite()?;
            let hi = rd.finite()?;
            if lo < c0 || hi > c1 || lo >= hi {
                return Err(bad("find interval: interval outside the curve domain"));
            }
            rd.finite()?;
            let d = rd.domain()?;
            if d[2] - d[0] < 1e-6 || d[3] - d[1] < 1e-6 {
                return Err(bad("find interval: trim domain too small"));
            }
        }
        Op::Sing => {
            let (_, srf) = wrapped(rd)?;
            uv_in(rd, &srf)?;
            rd.vec3()?;
            let v = rd.flag()?;
            rd.finite()?;
            let s = rd.finite()?;
            let d = srf.dom;
            let (a, b) = if v { (d[1], d[3]) } else { (d[0], d[2]) };
            if s < a || s > b {
                return Err(bad("singular param outside the surface domain"));
            }
        }
        Op::Seam => {
            wrapped(rd)?;
            rd.finites(4)?;
            rd.domain()?;
        }
        Op::IsoLen => {
            let (_, srf) = wrapped(rd)?;
            uv_in(rd, &srf)?;
        }
        Op::Step => {
            sized_surface(rd)?;
            rd.flag()?;
            rd.flag()?;
            let lo = rd.finite()?;
            let hi = rd.finite()?;
            if lo > hi {
                return Err(bad("step: interval not ordered"));
            }
        }
        Op::Cache => {
            wrapped(rd)?;
            rd.flag()?;
        }
        Op::BBox => {
            let (wrap, kind, srf, _) = wrapped_full(rd)?;
            rd.domain_in(&srf, false)?;
            let normal = rd.flag()?;
            let pseudo = rd.flag()?;
            rd.flag()?;
            let polar = rd.flag()?;
            // upstream asserts that a box is asked for
            if !normal && !pseudo && !polar {
                return Err(bad("bounding box: ask for a box"));
            }
            // the base body recurses without bound on a polar box (UB-84)
            if polar && (wrap || kind == Kind::CurveBounded) {
                return Err(bad(
                    "bounding box: no polar box of curve-bounded or STEP surfaces (UB-84)",
                ));
            }
        }
        Op::DomCont => {
            let (_, srf) = wrapped(rd)?;
            rd.index(MAX_CONTINUITY)?;
            uv_in(rd, &srf)?;
            rd.finites(2)?;
        }
        Op::Sil => {
            let (kind, srf, size) = sized_surface(rd)?;
            if kind == Kind::CurveBounded {
                return Err(bad("silhouette: a B-spline or a plane"));
            }
            rd.domain_in(&srf, true)?;
            let persp = rd.flag()?;
            let eye = rd.vec3()?;
            if !persp && eye.iter().all(|x| *x == 0.0) {
                return Err(bad("silhouette: zero view vector (upstream asserts)"));
            }
            let has_tol = rd.flag()?;
            let tol = rd.finite()?;
            if has_tol && tol < MIN_SIL_TOL * size {
                return Err(bad("silhouette: approximation tolerance too small"));
            }
            let has_ang = rd.flag()?;
            let ang = rd.finite()?;
            if has_ang && !(MIN_SIL_ANG..=core::f64::consts::PI).contains(&ang) {
                return Err(bad("silhouette: angle tolerance outside [1e-3, pi]"));
            }
            rd.flag()?;
            rd.vec3()?;
        }
    }
    Ok(())
}

/// Checks `input` for `op` (layouts in the module docs).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for input the shim must not see.
pub fn validate(op: Op, input: &[f64]) -> Result<(), FfiError> {
    let mut rd = Reader { input, at: 0 };
    validate_op(op, &mut rd)?;
    rd.done()
}

/// Runs `op` on `input` (layouts in the module docs); returns the shim's output record.
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for input the shim must not see; [`FfiError::Status`]
/// for a shim failure.
pub fn eval(op: Op, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    validate(op, input)?;
    call_with_buffer(65_536, |buf, len| {
        // SAFETY: input is valid for input.len() reads and holds exactly what the shim
        // reads for `op` (validated above: well-formed clamped surfaces, planes with
        // independent axes, points and domains inside the surface domain where upstream
        // evaluates, derivative orders the shim's array holds, valid enumerators); buf is
        // valid for buf.len() writes and the shim honours the capacity; len is valid.
        unsafe {
            sys::sc_surface_surface_p1_eval(
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
