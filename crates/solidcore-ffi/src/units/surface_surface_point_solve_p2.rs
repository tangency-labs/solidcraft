//! FFI wrappers for unit `surface.surface_point_solve_p2`.
//!
//! Shim: `harness/cpp/shim/surface_surface_point_solve_p2.cpp`
//! (`sc_surface_surface_point_solve_p2_eval`, which compiles the upstream file with its
//! file-local classes renamed). Upstream: `source/SMLib/src/SmSurfacePointSolve.cpp`.
//!
//! Notation as in [`super::surface_surface_point_solve_p1`]: every op starts with the
//! problem `P = W S D op test(3) tol`; `uv` is a point `u v`, `D` a domain
//! `umin vmin umax vmax`. The stepper ops continue with `uv0 tight`: the stepper
//! `SmSurfPtSolveStepper(S, D, test, uv0, tight, tol, op)`.
//!
//! | [`Op`]     | upstream                                                 | parameters             |
//! |------------|----------------------------------------------------------|------------------------|
//! | `Rough`    | `SmSurfPtGlobalSolver::FindRoughGuess`                   | `P`                    |
//! | `Poly`     | `FindPolygonGuess` (from best indices `iu iv`)           | `P iu iv`              |
//! | `Refine`   | `RefineGuess`                                            | `P uv`                 |
//! | `Patch`    | `RefinePatchByGrid`                                      | `P sub nu nv uv best`  |
//! | `Grid`     | `RefineGuessByGrid`                                      | `P uv iu iv`           |
//! | `Seams`    | `CheckSeams` of the solution `(u, v)` with `value`       | `P uv value`           |
//! | `Solve`    | `Solve`                                                  | `P`                    |
//! | `Adjust`   | `AdjustStep` of a copy moved to `uv1` (after `hu`/`hv` boundary hits) | `P uv0 tight uv1 hu hv` |
//! | `Norms`    | the nine `ErrorNorm`s, `IsSingular`                      | `P uv0 tight`          |
//! | `Conv`     | `Converged(ctol)`, `Converged(TRUE)`, `Converged(FALSE)` | `P uv0 tight ctol`     |
//! | `Push`     | `PushingBoundary(ptol)`, `ConvergedOnBoundary(ptol)`     | `P uv0 tight ptol`     |
//! | `Snap`     | `SnapToBoundaries(stol)`                                 | `P uv0 tight stol`     |
//! | `Step`     | `Step` of a copy, Newton or plane                        | `P uv0 tight plane`    |
//! | `Line`     | `LineSearch` of a copy moved to `uv1`                    | `P uv0 tight uv1`      |
//! | `LineB`    | `LineSearchBracketted` of a copy moved to `uv1`          | `P uv0 tight uv1 norm` |
//!
//! The output is the op status (`0` = `SM_SUCCESS`, else the `SmStatus`) and the op's
//! outputs (see the shim). [`eval`] accepts only what upstream handles without undefined
//! behaviour: the problems of [`super::surface_surface_point_solve_p1`], points inside the
//! surface domain wherever a surface evaluator reads them (the step target of `Adjust`,
//! which is only clamped, within a domain size of it), a grid sub-domain inside the
//! surface domain with grid sizes up to [`MAX_GRID`], control-net indices inside the net,
//! boundary-hit counts up to 3, an `SmSrfPtErrorNormType`, finite values.
//!
//! [`eval_rev`] runs an op on a problem over a surface of revolution (surface kind `3`,
//! the block of [`super::surface_surf_of_revolution_p1`], checked there): the shim
//! first builds the surface and reports its natural domain and control-point counts
//! ([`rev_info`]), against which the rest of the input is checked as above.

use super::surface_surf_of_revolution_p1::{self as rev, Surf as RevSurf};
use super::surface_surface_cache_p1::{bad, Reader, Srf};
use super::surface_surface_point_solve_p1::{net_index, problem, uv_in, uv_near, Problem, OPS};
use crate::{call_with_buffer, FfiError};

/// The upstream routine run by the shim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// `FindRoughGuess`.
    Rough,
    /// `FindPolygonGuess`.
    Poly,
    /// `RefineGuess`.
    Refine,
    /// `RefinePatchByGrid`.
    Patch,
    /// `RefineGuessByGrid`.
    Grid,
    /// `CheckSeams`.
    Seams,
    /// `Solve`.
    Solve,
    /// `AdjustStep`.
    Adjust,
    /// `ErrorNorm`, `IsSingular`.
    Norms,
    /// `Converged`.
    Conv,
    /// `PushingBoundary`, `ConvergedOnBoundary`.
    Push,
    /// `SnapToBoundaries`.
    Snap,
    /// `Step`.
    Step,
    /// `LineSearch`.
    Line,
    /// `LineSearchBracketted`.
    LineB,
}

impl Op {
    /// The shim's op code.
    #[must_use]
    pub fn code(self) -> i32 {
        match self {
            Op::Rough => 0,
            Op::Poly => 1,
            Op::Refine => 2,
            Op::Patch => 3,
            Op::Grid => 4,
            Op::Seams => 5,
            Op::Solve => 6,
            Op::Adjust => 7,
            Op::Norms => 8,
            Op::Conv => 9,
            Op::Push => 10,
            Op::Snap => 11,
            Op::Step => 12,
            Op::Line => 13,
            Op::LineB => 14,
        }
    }
}

/// The shim's op that builds a surface of revolution and reports its domain and
/// control-point counts (see [`eval_rev`]).
const OP_REV_INFO: i32 = 15;

/// Calls the shim with `code` on `input`.
fn call(code: i32, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    call_with_buffer(4096, |buf, len| {
        // SAFETY: input is valid for input.len() reads and holds exactly what the shim
        // reads for `code` (validated by the callers: well-formed clamped surfaces,
        // planes with independent axes, surfaces of revolution as
        // `surface_surf_of_revolution_p1` accepts them, a solver domain inside the
        // surface domain, supported operations, evaluated points and grid sub-domains
        // inside the surface domain, control-net indices inside the net, bounded grid
        // sizes and hit counts, a valid norm type); buf is valid for buf.len() writes
        // and the shim honours the capacity; len is valid.
        unsafe {
            sys::sc_surface_surface_point_solve_p2_eval(
                code,
                input.as_ptr(),
                input.len() as i64,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}

/// Largest grid size of `Patch` (sizes below 2 count as 2 upstream).
pub const MAX_GRID: i64 = 24;
/// Largest boundary-hit count of `Adjust` (either sign).
pub const MAX_HITS: i64 = 3;
/// Largest `SmSrfPtErrorNormType` (`SM_SPN_GAP`).
pub const MAX_NORM: usize = 8;

mod sys {
    extern "C" {
        pub fn sc_surface_surface_point_solve_p2_eval(
            op: i32,
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

/// An integer in `-max..=max`.
fn small_int(rd: &mut Reader<'_>, max: i64, what: &str) -> Result<(), FfiError> {
    let v = rd.num()?;
    if v.fract() != 0.0 || v.abs() > max as f64 {
        return Err(bad(format!("{what}: bad integer {v}")));
    }
    Ok(())
}

fn validate_op(op: Op, rd: &mut Reader<'_>) -> Result<(), FfiError> {
    let p = problem(rd)?;
    validate_params(op, rd, &p)
}

/// The rest of a problem after its surface `W S`: `D op test(3) tol`, as
/// [`super::surface_surface_point_solve_p1`]'s problem.
fn problem_tail(rd: &mut Reader<'_>, srf: &Srf) -> Result<(), FfiError> {
    rd.domain_in(srf, true)?;
    let op = rd.index(10)?;
    if !OPS.contains(&op) {
        return Err(bad(format!("unsupported solver operation {op}")));
    }
    rd.vec3()?;
    rd.finite()?;
    Ok(())
}

/// The parameters of `op` after the problem `p`.
fn validate_params(op: Op, rd: &mut Reader<'_>, p: &Problem) -> Result<(), FfiError> {
    match op {
        Op::Rough | Op::Solve => {}
        Op::Poly => net_index(rd, p, 0)?,
        Op::Refine => uv_in(rd, &p.srf)?,
        Op::Patch => {
            rd.domain_in(&p.srf, false)?;
            small_int(rd, MAX_GRID, "grid size")?;
            small_int(rd, MAX_GRID, "grid size")?;
            rd.finite()?;
            rd.finite()?;
            if rd.num()?.is_nan() {
                return Err(bad("best value is NaN"));
            }
        }
        Op::Grid => {
            uv_in(rd, &p.srf)?;
            net_index(rd, p, 0)?;
        }
        Op::Seams => {
            uv_in(rd, &p.srf)?;
            rd.finite()?;
        }
        Op::Adjust | Op::Norms | Op::Conv | Op::Push | Op::Snap | Op::Step | Op::Line | Op::LineB => {
            uv_in(rd, &p.srf)?;
            rd.finite()?;
            match op {
                Op::Adjust => {
                    uv_near(rd, &p.srf)?;
                    small_int(rd, MAX_HITS, "boundary hits")?;
                    small_int(rd, MAX_HITS, "boundary hits")?;
                }
                Op::Conv | Op::Push | Op::Snap => {
                    rd.finite()?;
                }
                Op::Step => {
                    rd.flag()?;
                }
                Op::Line => uv_in(rd, &p.srf)?,
                Op::LineB => {
                    uv_in(rd, &p.srf)?;
                    rd.index(MAX_NORM)?;
                }
                _ => {}
            }
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
    call(op.code(), input)
}

/// A built surface of revolution: the natural UV domain `umin vmin umax vmax` of the
/// (wrapped) surface and the control-point counts in u and v.
pub type RevInfo = ([f64; 4], usize, usize);

/// The surface of revolution `surf` (wrapped in an `SmSTEPSurface` when `wrap`) as the
/// shim builds it: its construction status and, when it was built, the natural UV domain
/// of the (wrapped) surface and the control-point counts of the surface of revolution.
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for a surface the shim must not see
/// (`surface_surf_of_revolution_p1`'s checks); [`FfiError::Status`] for a shim failure.
pub fn rev_info(wrap: bool, surf: &RevSurf) -> Result<(i32, Option<RevInfo>), FfiError> {
    let head = rev_head(wrap, surf)?;
    let out = call(OP_REV_INFO, &head)?;
    let status = out.first().copied().unwrap_or(f64::NAN);
    if status != 0.0 {
        return Ok((status as i32, None));
    }
    match out.get(1..7) {
        Some(&[a, b, c, d, nu, nv]) => Ok((0, Some(([a, b, c, d], nu as usize, nv as usize)))),
        _ => Err(bad("short info record")),
    }
}

/// `W 3 R`: the problem's surface block for a surface of revolution.
pub(crate) fn rev_head(wrap: bool, surf: &RevSurf) -> Result<Vec<f64>, FfiError> {
    let mut head = vec![f64::from(u8::from(wrap)), 3.0];
    rev::push_surf(&mut head, surf)?;
    Ok(head)
}

/// Checks a problem over the surface of revolution `surf` (wrapped in an
/// `SmSTEPSurface` when `wrap`) for `op`: `rest` is the problem after its surface
/// (`D op test tol`) and the op's parameters, checked against the domain and
/// control-point counts the shim reports for the surface (see [`rev_info`]; this builds
/// the surface in C++).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for input the shim must not see (including a surface
/// upstream cannot build); [`FfiError::Status`] for a shim failure.
pub fn validate_rev(op: Op, wrap: bool, surf: &RevSurf, rest: &[f64]) -> Result<(), FfiError> {
    let (status, info) = rev_info(wrap, surf)?;
    let Some((dom, num_u, num_v)) = info else {
        return Err(bad(format!("surface of revolution not built (status {status})")));
    };
    let ordered = dom.iter().all(|v| v.is_finite()) && dom[0] < dom[2] && dom[1] < dom[3];
    if !ordered {
        return Err(bad("surface of revolution: bad domain"));
    }
    let p = Problem {
        srf: Srf {
            dom,
            u_distinct: 0,
            v_distinct: 0,
        },
        num_u,
        num_v,
    };
    let mut rd = Reader { input: rest, at: 0 };
    problem_tail(&mut rd, &p.srf)?;
    validate_params(op, &mut rd, &p)?;
    rd.done()
}

/// Runs `op` on a problem over the surface of revolution `surf` (see [`validate_rev`]);
/// returns the shim's output record.
///
/// # Errors
///
/// Those of [`validate_rev`]; [`FfiError::Status`] for a shim failure.
pub fn eval_rev(op: Op, wrap: bool, surf: &RevSurf, rest: &[f64]) -> Result<Vec<f64>, FfiError> {
    validate_rev(op, wrap, surf, rest)?;
    let mut input = rev_head(wrap, surf)?;
    input.extend_from_slice(rest);
    call(op.code(), &input)
}
