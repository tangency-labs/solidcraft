//! FFI wrappers for unit `surface.surface_point_solve_p1`.
//!
//! Shim: `harness/cpp/shim/surface_surface_point_solve_p1.cpp`
//! (`sc_surface_surface_point_solve_p1_eval`, which compiles the upstream file with its
//! file-local classes renamed). Upstream: `source/SMLib/src/SmSurfacePointSolve.cpp`.
//!
//! Notation as in [`super::surface_surface_p1`]: `W S` a surface (`S`: `0 B` B-spline
//! `oob form N`, `1 o x y sx sy D` bounded `SmPlane`, `2 B` `SmCurveBoundedSurface`),
//! wrapped in an `SmSTEPSurface` when `W = 1`; `uv` a point `u v`; `D` a domain
//! `umin vmin umax vmax`. Every op starts with the problem `P = W S D op test(3) tol`
//! (`op` an `SmSolverOperationType`: 0 intersect, 3 minimise, 4 maximise, 5 normalise,
//! 10 3D signed directed maximise; `D` the solver domain).
//!
//! | [`Op`]        | upstream                                              | parameters        |
//! |---------------|-------------------------------------------------------|-------------------|
//! | `Ctor`        | `SmSurfacePointSolver` constructor (via the global solver) | `P`          |
//! | `Closed`      | `SmSurfPtGlobalSolver::ClosedSurface` (U, V, U, BOTH)  | `P`               |
//! | `Func`        | `CalcFuncVal`                                         | `P uv`            |
//! | `TriGuess`    | `FindPolygonTriangleGuess`                            | `P iu iv`         |
//! | `Leg`         | `RefineGuessPolygonLeg`                               | `P iu iv`         |
//! | `Stride`      | `CheckBadGuessStride`                                 | `P uv`            |
//! | `SeamsUv`     | `CheckGuessForSeams` (UV point)                       | `P uv`            |
//! | `SeamsCpts`   | `CheckGuessForSeams` (control points)                 | `P iu iv`         |
//! | `Crease`      | `GetCreaseSubdomains`                                 | `P`               |
//! | `Local`       | `IsLocalMin`, `IsLocalMax`, `PushingBoundary`         | `P uv tol`        |
//! | `TriDist`     | `GetTrianglesDist` (static)                           | `P i j`           |
//! | `Well`        | `WellBehavedControlNet` (static)                      | `P i j`           |
//! | `Stepper`     | `SmSurfPtSolveStepper`: constructor, `MaxStep`, `Eval` values, `Pos` and derivatives, copy, `operator=`, `SetUV`, `Clamp` | `P uv tight uv2` |
//! | `HitBound`    | `HitBound` after `SetUV`                              | `P uv k (uv which tob)*k` |
//! | `CheckBounds` | `CheckBounds`                                         | `P curr next D`   |
//! | `DropIso`     | `DropToIsoCurve`                                      | `P uv which crease param` |
//!
//! The output is the op status (`0` = `SM_SUCCESS`, else the `SmStatus`) and the op's
//! outputs (see the shim). [`eval`] accepts only what upstream handles without undefined
//! behaviour: the surfaces of [`super::surface_surface_cache_p3`], a solver domain of
//! positive size inside the surface domain, the operations above, points inside the
//! surface domain wherever a surface evaluator is read (within a domain size of it for
//! `CalcFuncVal`, which checks the evaluation's status), control-net indices inside the
//! net (a plane's net is 2 x 2), finite values.

use super::surface_surface_cache_p1::{bad, Reader, Srf};
use super::surface_surface_cache_p3::{sized_surface, Kind};
use crate::{call_with_buffer, FfiError};

/// The upstream routine run by the shim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// The constructor's problem data.
    Ctor,
    /// `ClosedSurface`.
    Closed,
    /// `CalcFuncVal`.
    Func,
    /// `FindPolygonTriangleGuess`.
    TriGuess,
    /// `RefineGuessPolygonLeg`.
    Leg,
    /// `CheckBadGuessStride`.
    Stride,
    /// `CheckGuessForSeams` (UV point).
    SeamsUv,
    /// `CheckGuessForSeams` (control points).
    SeamsCpts,
    /// `GetCreaseSubdomains`.
    Crease,
    /// `IsLocalMin`, `IsLocalMax`, `PushingBoundary`.
    Local,
    /// `GetTrianglesDist`.
    TriDist,
    /// `WellBehavedControlNet`.
    Well,
    /// The stepper's construction, evaluation and copies.
    Stepper,
    /// `HitBound`.
    HitBound,
    /// `CheckBounds`.
    CheckBounds,
    /// `DropToIsoCurve`.
    DropIso,
}

impl Op {
    /// The shim's op code.
    #[must_use]
    pub fn code(self) -> i32 {
        match self {
            Op::Ctor => 0,
            Op::Closed => 1,
            Op::Func => 2,
            Op::TriGuess => 3,
            Op::Leg => 4,
            Op::Stride => 5,
            Op::SeamsUv => 6,
            Op::SeamsCpts => 7,
            Op::Crease => 8,
            Op::Local => 9,
            Op::TriDist => 10,
            Op::Well => 11,
            Op::Stepper => 12,
            Op::HitBound => 13,
            Op::CheckBounds => 14,
            Op::DropIso => 15,
        }
    }
}

/// Most `HitBound` steps in one case.
pub const MAX_HIT_STEPS: usize = 64;

/// The solver operations the file's classes accept.
pub(crate) const OPS: [usize; 5] = [0, 3, 4, 5, 10];

mod sys {
    extern "C" {
        pub fn sc_surface_surface_point_solve_p1_eval(
            op: i32,
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

/// The problem `P`: the surface domain and the control-net counts (`2 x 2` for a plane).
pub(crate) struct Problem {
    pub(crate) srf: Srf,
    pub(crate) num_u: usize,
    pub(crate) num_v: usize,
}

pub(crate) fn problem(rd: &mut Reader<'_>) -> Result<Problem, FfiError> {
    rd.flag()?;
    let at = rd.at;
    let (kind, srf, _) = sized_surface(rd)?;
    let get = |i: usize| rd.input.get(i).copied().unwrap_or(0.0) as usize;
    let (num_u, num_v) = match kind {
        Kind::Plane => (2, 2),
        // `kind oob form n m ..`
        Kind::BSpline | Kind::CurveBounded => (get(at + 3) + 1, get(at + 4) + 1),
    };
    rd.domain_in(&srf, true)?;
    let op = rd.index(10)?;
    if !OPS.contains(&op) {
        return Err(bad(format!("unsupported solver operation {op}")));
    }
    rd.vec3()?;
    rd.finite()?;
    Ok(Problem { srf, num_u, num_v })
}

/// A point `uv` inside the surface domain.
pub(crate) fn uv_in(rd: &mut Reader<'_>, srf: &Srf) -> Result<(), FfiError> {
    let u = rd.finite()?;
    let v = rd.finite()?;
    let d = srf.dom;
    if u < d[0] || u > d[2] || v < d[1] || v > d[3] {
        return Err(bad("uv outside the surface domain"));
    }
    Ok(())
}

/// A point `uv` within a domain size of the surface domain.
pub(crate) fn uv_near(rd: &mut Reader<'_>, srf: &Srf) -> Result<(), FfiError> {
    let u = rd.finite()?;
    let v = rd.finite()?;
    let d = srf.dom;
    let (w, h) = (d[2] - d[0], d[3] - d[1]);
    if u < d[0] - w || u > d[2] + w || v < d[1] - h || v > d[3] + h {
        return Err(bad("uv too far outside the surface domain"));
    }
    Ok(())
}

/// Control-net indices `i j` with `i + du < num_u`, `j + dv < num_v`.
pub(crate) fn net_index(rd: &mut Reader<'_>, p: &Problem, d: usize) -> Result<(), FfiError> {
    let i = rd.index(1 << 20)?;
    let j = rd.index(1 << 20)?;
    if i + d >= p.num_u || j + d >= p.num_v {
        return Err(bad("control point index past the net"));
    }
    Ok(())
}

fn validate_op(op: Op, rd: &mut Reader<'_>) -> Result<(), FfiError> {
    let p = problem(rd)?;
    match op {
        Op::Ctor | Op::Closed | Op::Crease => {}
        Op::Func => uv_near(rd, &p.srf)?,
        Op::TriGuess | Op::Leg | Op::SeamsCpts | Op::Well => net_index(rd, &p, 0)?,
        Op::TriDist => net_index(rd, &p, 1)?,
        Op::Stride | Op::SeamsUv => uv_in(rd, &p.srf)?,
        Op::Local => {
            uv_in(rd, &p.srf)?;
            rd.finite()?;
        }
        Op::Stepper => {
            uv_in(rd, &p.srf)?;
            rd.finite()?;
            rd.finite()?;
            rd.finite()?;
        }
        Op::HitBound => {
            uv_in(rd, &p.srf)?;
            let k = rd.index(MAX_HIT_STEPS)?;
            for _ in 0..k {
                rd.finite()?;
                rd.finite()?;
                rd.flag()?;
                let tob = rd.finite()?;
                if tob.fract() != 0.0 || tob.abs() > 3.0 {
                    return Err(bad("hit bound: direction must be a small integer"));
                }
            }
        }
        Op::CheckBounds => {
            uv_in(rd, &p.srf)?;
            rd.finite()?;
            rd.finite()?;
            rd.domain()?;
        }
        Op::DropIso => {
            uv_in(rd, &p.srf)?;
            let v = rd.flag()?;
            let c = rd.finite()?;
            let (lo, hi) = if v {
                (p.srf.dom[1], p.srf.dom[3])
            } else {
                (p.srf.dom[0], p.srf.dom[2])
            };
            if c < lo || c > hi {
                return Err(bad("drop to iso curve: crease value outside the surface domain"));
            }
            rd.finite()?;
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
    call_with_buffer(4096, |buf, len| {
        // SAFETY: input is valid for input.len() reads and holds exactly what the shim
        // reads for `op` (validated above: well-formed clamped surfaces, planes with
        // independent axes, a solver domain inside the surface domain, supported
        // operations, evaluated points inside the surface domain, control-net indices
        // inside the net, bounded step counts); buf is valid for buf.len() writes and the
        // shim honours the capacity; len is valid.
        unsafe {
            sys::sc_surface_surface_point_solve_p1_eval(
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
