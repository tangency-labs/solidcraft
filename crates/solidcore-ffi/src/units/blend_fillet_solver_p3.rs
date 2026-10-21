//! FFI wrappers for unit `blend.fillet_solver_p3`.
//!
//! Shim: `harness/cpp/shim/blend_fillet_solver_p3.cpp` (`sc_blend_fillet_solver_p3_eval`).
//! Upstream: `source/SMLib/src/SmFilletSolver.cpp` (`SmFilletSolver::StepSolve`).
//!
//! Surface blocks are those of unit `topology.edgeuse_p1` (`1 O[3] N[3]` an `SmPlane`,
//! `0 oob form n m p q r s cpts u v` an `SmBSplineSurface`). The solver is the shim's
//! deterministic `SmFilletSolver` subclass (module docs of the shim): offset surfaces
//! `radius * orientation_i` of the two base surfaces, no edge uses or executive.
//!
//! | [`Op`]  | upstream    | parameters |
//! |---------|-------------|------------|
//! | `Step`  | `StepSolve` | `type tol radius o0 o1 dist extra init_fail allow_ext surf0 surf1 uv0[2] uv1[2] duv0[2] duv1[2] crv_pos[3] crv_deriv[3] step dom1[4] dom2[4] next_uv0[2] next_uv1[2] next_ud[4]` |
//!
//! Domains are `umin vmin umax vmax`. Output: the status, then on success `found clipped
//! boundary step next_uv0[2] next_uv1[2] next_ud[4] offset0 offset1`.
//!
//! [`eval`] accepts only what upstream handles without undefined behaviour: valid surface
//! blocks, finite numbers, a positive tolerance, ordered domains and a solver type of
//! `SmFilletSolverType` other than `SM_FS_VARIABLE_RADIUS` (`StepSolve` casts a variable
//! radius solver to `SmVariableRadiusFS`, which the shim's solver is not).

use crate::units::topology_edgeuse_p1::{bad, surface, Reader};
use crate::{call_with_buffer, FfiError};

/// `SM_FS_VARIABLE_RADIUS`.
const VARIABLE_RADIUS: usize = 3;
/// Largest `SmFilletSolverType` value (`SM_FS_CONST_RADIUS_ASSISTED`).
const MAX_SOLVER_TYPE: usize = 6;

/// The upstream routine run by the shim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// `SmFilletSolver::StepSolve`.
    Step,
}

impl Op {
    const fn code(self) -> i32 {
        match self {
            Op::Step => 0,
        }
    }
}

mod sys {
    extern "C" {
        pub fn sc_blend_fillet_solver_p3_eval(
            op: i32,
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

/// `umin vmin umax vmax` with `umin <= umax`, `vmin <= vmax`.
fn domain(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    let d = rd.finites(4)?;
    if let [u0, v0, u1, v1] = d[..] {
        if u0 > u1 || v0 > v1 {
            return Err(bad("domain: min above max"));
        }
    }
    Ok(())
}

fn validate(op: Op, input: &[f64]) -> Result<(), FfiError> {
    let mut rd = Reader { input, at: 0 };
    match op {
        Op::Step => {
            let ty = rd.index(MAX_SOLVER_TYPE)?;
            if ty == VARIABLE_RADIUS {
                return Err(bad(
                    "step: a variable radius solver is cast to SmVariableRadiusFS",
                ));
            }
            let tol = rd.finite()?;
            if tol <= 0.0 {
                return Err(bad("step: tolerance must be positive"));
            }
            rd.finites(4)?; // radius, orientations, distance
            rd.flag()?;
            rd.flag()?;
            rd.flag()?;
            surface(&mut rd)?;
            surface(&mut rd)?;
            rd.finites(8 + 6 + 1)?; // current point, step
            domain(&mut rd)?;
            domain(&mut rd)?;
            rd.finites(8)?; // next point
        }
    }
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
    call_with_buffer(64, |buf, len| {
        // SAFETY: input is valid for input.len() reads and holds exactly what the shim
        // reads for `op` (validated above: well-formed surface blocks, every flag and
        // solver type the shim passes to upstream); buf is valid for buf.len() writes and
        // the shim honours the capacity; len is valid.
        unsafe {
            sys::sc_blend_fillet_solver_p3_eval(
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
