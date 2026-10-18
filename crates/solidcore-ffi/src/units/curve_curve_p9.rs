//! FFI wrappers for unit `curve.curve_p9`.
//!
//! Shim: `harness/cpp/shim/curve_curve_p9.cpp` (`sc_curve_curve_p9_eval`). Upstream:
//! `source/SMLib/src/SmCurve.cpp`, `source/SMLib/inc/SmCurve.h` (`SmCurve::GlobalCurveSolve`
//! with `SmCCGlobalSolver::LocalSolve` and `sm_SolveMinSignedAngle`,
//! `GlobalCurveSelfIntersect`, `CalculateTightBoundingBox`, `CalculateTightPseudoBox`,
//! `IsOnPlane`, `IsOnPositiveSideOfPlanes`).
//!
//! A curve block is `0 dim oob n p m cpts(x y z w) knots` (an `SmBSplineCurve` built with
//! `SmBSplineCurve(gw_CURVE*, dim, FALSE)` and `SetOutOfBoundsEnabled(oob)`), `1 dim P1[3]
//! D1[3] P2[3] D2[3]` (an `SmHermiteCurve`) or `2 3 P[3] V[3] d0 d1 scale io` (a bounded
//! `SmLine(P, V, [d0, d1], scale, 3, ctx, NULL, io)`) or `3 3 C[3] X[3] Y[3] d0 d1 rx ry io`
//! (an `SmEllipse` without source NURB, as in unit `curve.curve_p5`), as in unit
//! `curve.curve_p8`. Every
//! output starts with the status (`0` = `SM_SUCCESS`, else the `SmStatus`); the rest (on
//! success):
//!
//! | [`Op`]           | upstream                     | parameters                                                   | outputs                  |
//! |------------------|------------------------------|--------------------------------------------------------------|--------------------------|
//! | `Gcs`            | `GlobalCurveSolve`           | curve `a b same [curve] a2 b2 op tol has_target target nvec vec*nvec requested` | `n sol*n` |
//! | `SelfIntersect`  | `GlobalCurveSelfIntersect`   | curve `a b tol`                                              | `n sol*n`                |
//! | `TightBox`       | `CalculateTightBoundingBox`  | curve `a b`                                                  | `umin vmin wmin umax vmax wmax` |
//! | `TightPseudoBox` | `CalculateTightPseudoBox`    | curve `a b B0[3] B1[3] B2[3]`                                | `basis[9] (min max)*3`   |
//! | `OnPlane`        | `IsOnPlane`                  | curve `P[3] N[3] tol has_ivl a b`                            | `on max_dist`            |
//! | `PositiveSide`   | `IsOnPositiveSideOfPlanes`   | curve `n (P[3] N[3])*n`                                      | `positive` (status `0`)  |
//!
//! `same` passes this curve itself as the other one (no second block); `op` is an
//! `SmSolverOperationType` from [`PLAIN_OPS`] (no vectors, `nvec` `0`) or
//! [`VECTOR_OPS`] (`nvec` `3`); `has_target` passes `&target` (required for
//! `SM_SO_AT_DISTANCE`); `requested` is `SM_SR_SINGLE` (`0`) or `SM_SR_ALL` (`1`). A
//! solution `sol` is `type nvar nobj o0 o1 value s0 s1 [evalue e0 e1]` (the end for a
//! range); objects are `0` (this curve), `1` (the other), `2` (another object), `-1`
//! (`NULL`). `IsOnPlane`'s `max_dist` starts as `SM_UNDEF_DOUBLE`; `has_ivl` `0` passes
//! `NULL` (`a b` are then ignored).
//!
//! [`eval`] accepts only what upstream handles without undefined behaviour or runaway
//! work: well-formed clamped B-splines (`m = n+p+1`, `1 <= p <= n`, `p <= 9`, finite
//! bounded data, positive weights, end multiplicity `p+1`, interior at most `p`) of
//! dimension 2 or 3, finite bounded Hermite data, lines with non-zero vectors and
//! positive scales, ellipses with non-parallel axes and sane radii and domains, parameters within a few curve lengths of the curve's range,
//! non-reversed intervals (upstream's `SmExtent1d` signals an error for a reversed one),
//! non-negative tolerances and finite vectors.

use crate::validate::{bad, euclid, NOW, NOZ};
use crate::{call_with_buffer, FfiError};

/// Largest control point index `n`.
pub const MAX_INDEX: usize = 128;
/// Largest degree.
pub const MAX_DEGREE: usize = 9;
/// Largest coordinate magnitude.
pub const MAX_COORD: f64 = 1e6;
/// Parameters may lie this many interval lengths outside the curve's range.
pub const MAX_REL_DIST: f64 = 2.0;

/// Shim operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// `SmCurve::GlobalCurveSolve`.
    Gcs,
    /// `SmCurve::GlobalCurveSelfIntersect`.
    SelfIntersect,
    /// `CalculateTightBoundingBox`.
    TightBox,
    /// `SmCurve::CalculateTightPseudoBox`.
    TightPseudoBox,
    /// `IsOnPlane`.
    OnPlane,
    /// `SmCurve::IsOnPositiveSideOfPlanes`.
    PositiveSide,
}

impl Op {
    /// The shim's op code.
    #[must_use]
    pub fn code(self) -> i32 {
        match self {
            Op::Gcs => 0,
            Op::SelfIntersect => 1,
            Op::TightBox => 2,
            Op::TightPseudoBox => 3,
            Op::OnPlane => 4,
            Op::PositiveSide => 5,
        }
    }
}

/// `SmSolverOperationType` codes `GlobalCurveSolve` accepts without option vectors.
pub const PLAIN_OPS: [i32; 4] = [3, 4, 5, 6];
/// `SmSolverOperationType` codes `GlobalCurveSolve` accepts that read the three
/// option vectors.
pub const VECTOR_OPS: [i32; 12] = [9, 11, 12, 13, 14, 15, 16, 17, 18, 21, 22, 23];
/// `SM_SO_AT_DISTANCE`.
pub const AT_DISTANCE: i32 = 6;
/// Most planes of `IsOnPositiveSideOfPlanes`.
pub const MAX_PLANES: usize = 8;

mod sys {
    extern "C" {
        pub fn sc_curve_curve_p9_eval(
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

/// The parameter range of a validated curve block.
#[derive(Debug, Clone, Copy)]
struct CurveInfo {
    a: f64,
    b: f64,
}

/// A B-spline `n p m cpts knots`.
fn bspline(rd: &mut Reader<'_>) -> Result<(f64, f64), FfiError> {
    let n = rd.index(MAX_INDEX)?;
    let p = rd.index(MAX_DEGREE)?;
    let m = rd.index(MAX_INDEX + MAX_DEGREE + 1)?;
    if p == 0 || p > n || m != n + p + 1 {
        return Err(bad("curve: need 1 <= p <= n and m = n + p + 1"));
    }
    let cps = rd.take(4 * (n + 1))?;
    let knots = rd.take(m + 1)?;
    let (cpts, _) = cps.as_chunks::<4>();
    let [_, _, z0, w0] = *cpts.first().ok_or_else(|| bad("empty curve"))?;
    for &[x, y, z, w] in cpts {
        if (z == NOZ) != (z0 == NOZ) || (w == NOW) != (w0 == NOW) {
            return Err(bad("curve: mixed dimension or rationality"));
        }
        if !x.is_finite()
            || !y.is_finite()
            || (z != NOZ && !z.is_finite())
            || (w != NOW && !w.is_finite())
        {
            return Err(bad("curve: non-finite control point"));
        }
        if w != NOW && w <= 1e-3 {
            return Err(bad("curve: non-positive weight"));
        }
        if euclid([x, y, z, w])
            .iter()
            .any(|c| !c.is_finite() || c.abs() > MAX_COORD)
        {
            return Err(bad("curve: Euclidean point out of range"));
        }
    }
    if knots.iter().any(|k| !k.is_finite() || k.abs() > MAX_COORD)
        || knots.windows(2).any(|w| w.first() > w.last())
    {
        return Err(bad("curve: knots not non-decreasing"));
    }
    let (Some(&a), Some(&b)) = (knots.first(), knots.last()) else {
        return Err(bad("empty knots"));
    };
    let short = || bad("curve: knots too short");
    let head = knots.get(..=p).ok_or_else(short)?;
    let tail = knots.get(m - p..).ok_or_else(short)?;
    let inner = knots.get(p + 1..m - p).ok_or_else(short)?;
    if a >= b || head.iter().any(|&k| k != a) || tail.iter().any(|&k| k != b) {
        return Err(bad("curve: knots not clamped"));
    }
    if b - a < 1e-6 * a.abs().max(b.abs()).max(1.0) {
        return Err(bad("curve: parameter range too small"));
    }
    if inner.iter().any(|&k| k == a || k == b) {
        return Err(bad("curve: end multiplicity above p + 1"));
    }
    let mut run = 1;
    for w in inner.windows(2) {
        run = if w.first() == w.last() { run + 1 } else { 1 };
        if run > p {
            return Err(bad("curve: interior multiplicity above the degree"));
        }
    }
    Ok((a, b))
}

/// A line `P V d0 d1 scale io`; its analytic range.
fn line(rd: &mut Reader<'_>) -> Result<(f64, f64), FfiError> {
    rd.vec3()?;
    rd.nonzero3()?;
    let d0 = rd.finite()?;
    let d1 = rd.finite()?;
    if (d1 - d0).abs() < 1e-6 {
        return Err(bad("line: domain too small"));
    }
    let scale = rd.finite()?;
    if scale <= 1e-6 {
        return Err(bad("line: scale not positive"));
    }
    rd.flag()?;
    Ok((d0.min(d1), d0.max(d1)))
}

/// An ellipse placement and domain `C X Y d0 d1` (degrees).
fn conic_frame(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    rd.vec3()?;
    let x = rd.nonzero3()?;
    let y = rd.nonzero3()?;
    let cross = [
        x[1] * y[2] - x[2] * y[1],
        x[2] * y[0] - x[0] * y[2],
        x[0] * y[1] - x[1] * y[0],
    ];
    if cross.iter().map(|c| c * c).sum::<f64>() < 1e-12 {
        return Err(bad("ellipse: parallel axes"));
    }
    let d0 = rd.finite()?;
    let d1 = rd.finite()?;
    if d0.abs() > 720.0 || d1.abs() > 720.0 {
        return Err(bad("ellipse: domain end beyond 720 degrees"));
    }
    if !(1e-4..=360.0).contains(&(d1 - d0)) {
        return Err(bad("ellipse: domain length outside [1e-4, 360]"));
    }
    Ok(())
}

fn radius(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    let r = rd.finite()?;
    if !(1e-6..=1e6).contains(&r) {
        return Err(bad("ellipse: radius outside [1e-6, 1e6]"));
    }
    Ok(())
}

/// A curve block (see the module docs).
fn curve_block(rd: &mut Reader<'_>) -> Result<CurveInfo, FfiError> {
    let kind = rd.index(3)?;
    let dim = rd.index(3)?;
    if dim < 2 {
        return Err(bad("curve block: dimension must be 2 or 3"));
    }
    match kind {
        0 => {
            rd.flag()?;
            let (a, b) = bspline(rd)?;
            Ok(CurveInfo { a, b })
        }
        1 => {
            for _ in 0..4 {
                rd.vec3()?;
            }
            Ok(CurveInfo { a: 0.0, b: 1.0 })
        }
        3 => {
            if dim != 3 {
                return Err(bad("ellipse block: dimension must be 3"));
            }
            conic_frame(rd)?;
            radius(rd)?;
            radius(rd)?;
            rd.flag()?;
            Ok(CurveInfo { a: -1e3, b: 1e3 })
        }
        _ => {
            if dim != 3 {
                return Err(bad("line block: dimension must be 3"));
            }
            // a line evaluates anywhere: only bound its parameters
            line(rd)?;
            Ok(CurveInfo {
                a: -MAX_COORD / 8.0,
                b: MAX_COORD / 8.0,
            })
        }
    }
}

/// A parameter within [`MAX_REL_DIST`] ranges of the curve's.
fn param(rd: &mut Reader<'_>, c: &CurveInfo) -> Result<f64, FfiError> {
    let t = rd.finite()?;
    let far = MAX_REL_DIST * (c.b - c.a);
    if t < c.a - far || t > c.b + far {
        return Err(bad("parameter too far outside the curve"));
    }
    Ok(t)
}

/// A non-reversed interval `a b` of parameters of `c`.
fn interval(rd: &mut Reader<'_>, c: &CurveInfo) -> Result<(f64, f64), FfiError> {
    let a = param(rd, c)?;
    let b = param(rd, c)?;
    if a > b {
        return Err(bad("interval reversed"));
    }
    Ok((a, b))
}

/// A non-negative finite tolerance.
fn tol(rd: &mut Reader<'_>) -> Result<f64, FfiError> {
    let v = rd.finite()?;
    if v < 0.0 {
        return Err(bad("tolerance negative"));
    }
    Ok(v)
}

/// Whether `v` has unit length (upstream's `SM_IS_ZERO(Length() - 1)` contract).
fn is_unit(v: [f64; 3]) -> bool {
    (v.iter().map(|c| c * c).sum::<f64>().sqrt() - 1.0).abs() <= 1e-9
}

/// Largest distance from the view plane at which the eye of
/// `SM_SO_PERSPECTIVE_INTERSECT` counts as lying on it.
const EYE_ON_PLANE_TOL: f64 = 1e-3;

/// The contract of the projected intersections (ops 16, 17 and 18): upstream
/// asserts unit vectors (`GlobalCurveSolve` asserts `|cpOptVectors[0]| == 1`,
/// `SmVector3d::PerspectiveProjectPointToPlane` a unit normal) and the solver does
/// not unitize the view-plane normal (16), the rotation axis and plane x axis (17)
/// or the plane normal (18), so non-unit ones are outside the contract. A
/// perspective eye on (or within [`EYE_ON_PLANE_TOL`] of) the view plane is also
/// excluded: Rust and C++ disagree there, see
/// <https://github.com/tangency-labs/solidcore/issues/59>.
fn check_projection_vectors(code: i32, vecs: &[[f64; 3]; 3]) -> Result<(), FfiError> {
    let [v0, v1, v2] = *vecs;
    let units_ok = match code {
        16 => is_unit(v0),
        17 => is_unit(v1) && is_unit(v2),
        18 => is_unit(v1),
        _ => return Ok(()),
    };
    if !units_ok {
        return Err(bad("projection option vectors must be unit vectors"));
    }
    if code == 18 {
        let dist = (v2[0] - v0[0]) * v1[0] + (v2[1] - v0[1]) * v1[1] + (v2[2] - v0[2]) * v1[2];
        if dist.abs() <= EYE_ON_PLANE_TOL {
            return Err(bad(
                "perspective eye on the view plane (solidcore issue #59)",
            ));
        }
    }
    Ok(())
}

fn validate_op(op: Op, rd: &mut Reader<'_>) -> Result<(), FfiError> {
    match op {
        Op::Gcs => {
            let c = curve_block(rd)?;
            interval(rd, &c)?;
            let same = rd.flag()?;
            let c2 = if same { c } else { curve_block(rd)? };
            interval(rd, &c2)?;
            let code = rd.index(64)? as i32;
            let needs_vectors = VECTOR_OPS.contains(&code);
            if !needs_vectors && !PLAIN_OPS.contains(&code) {
                return Err(bad(format!("unsupported solver operation {code}")));
            }
            tol(rd)?;
            let has_target = rd.flag()?;
            rd.finite()?;
            if code == AT_DISTANCE && !has_target {
                return Err(bad("SM_SO_AT_DISTANCE needs a target distance"));
            }
            let nvec = rd.index(3)?;
            if nvec != if needs_vectors { 3 } else { 0 } {
                return Err(bad("wrong option vector count for the operation"));
            }
            let mut vecs = [[0.0; 3]; 3];
            for v in vecs.iter_mut().take(nvec) {
                *v = rd.vec3()?;
            }
            check_projection_vectors(code, &vecs)?;
            rd.index(1)?;
        }
        Op::SelfIntersect => {
            let c = curve_block(rd)?;
            interval(rd, &c)?;
            tol(rd)?;
        }
        Op::TightBox => {
            let c = curve_block(rd)?;
            interval(rd, &c)?;
        }
        Op::TightPseudoBox => {
            let c = curve_block(rd)?;
            interval(rd, &c)?;
            for _ in 0..3 {
                rd.vec3()?;
            }
        }
        Op::OnPlane => {
            let c = curve_block(rd)?;
            rd.vec3()?;
            rd.vec3()?;
            rd.finite()?;
            if rd.flag()? {
                interval(rd, &c)?;
            } else {
                rd.finite()?;
                rd.finite()?;
            }
        }
        Op::PositiveSide => {
            curve_block(rd)?;
            let n = rd.index(MAX_PLANES)?;
            for _ in 0..n {
                rd.vec3()?;
                rd.vec3()?;
            }
        }
    }
    Ok(())
}

/// Checks that `input` is a well-formed record for `op` (see the module docs).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] describing the first problem.
pub fn validate(op: Op, input: &[f64]) -> Result<(), FfiError> {
    let mut rd = Reader::new(input);
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
    call_with_buffer(1024, |buf, len| {
        // SAFETY: input is valid for input.len() reads and holds exactly what the shim
        // reads for `op` (validated above: well-formed curve blocks, every index, count
        // and flag the shim passes to upstream, at most three option vectors); buf is
        // valid for buf.len() writes and the shim honours the capacity; len is valid.
        unsafe {
            sys::sc_curve_curve_p9_eval(
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
