//! FFI wrappers for unit `surface.surface_p3`.
//!
//! Shim: `harness/cpp/shim/surface_surface_p3.cpp` (`sc_surface_surface_p3_eval`).
//! Upstream: `source/SMLib/src/SmSurface.cpp`, `source/SMLib/inc/SmSurface.h`.
//!
//! Every input starts with a surface block as in unit `surface.surface_p7`
//! (`super::surface_surface_p7`): `0 oob form n m p q r s cpts(x y z w) u v` (a B-spline
//! surface), `1 O[3] N[3]` (a plane) or `2 basis implicit` (a curve-bounded surface
//! without boundary curves). A domain is `u0 v0 u1 v1`; `pts` is `count` then `count`
//! parameter vectors of the equation set's size; `vecs` is `n` then `n` vectors. Every
//! output starts with the status (`0` = `SM_SUCCESS`, else the `SmStatus`); the rest:
//!
//! | [`Op`]           | upstream                                  | parameters after the surface                          | outputs |
//! |------------------|-------------------------------------------|-------------------------------------------------------|---------|
//! | `EvalGeometric`  | `SmSurface::EvaluateGeometric`            | `u v fromLeftU fromLeftV`                             | `K H k1 k2 EFG[3] LMN[3] V1[3] V2[3]` |
//! | `NormalSection`  | `SmSurface::EvaluateNormalSection`        | `u v fromLeftU fromLeftV dir[3]`                      | `T[3] k N[3]` |
//! | `Singularities`  | `SmSurface::GetSingularities`             | `wantPoints wantNormals`                              | `singular approx [P[3]*4] [N[3]*4]` |
//! | `Cover`          | `SmSurface::CoverCoincidentSurface`       | `other tol hasDom dom[4]`                             | `ok maxDist [covered dom[4]] stepDom[4]` |
//! | `ShapeMatch`     | `smsurf_CheckShapeMatch`                  | `other uv0 uv1 tol angTol`                            | `match` |
//! | `FindPsNormal`   | `SmFindPSNormalENFO::Evaluate`            | `P[3] squaredTol pts[2]`                              | per point `st [found F[2] J[4]]` |
//! | `SssIntersect`   | `SmSSSIntersectENFO::Evaluate`            | `s2 s3 pts[6]`                                        | per point `st [found F[6] J[36]]` |
//! | `LocalPlane`     | `SmSurface::LocalPlaneSurfaceIntersect`   | `other dom1 dom2 tol g1[2] g2[2] O[3] N[3]`           | `found [value u0 v0 u1 v1]` |
//! | `LocalSolve`     | `SmSurface::LocalSurfaceSolve`            | `other dom1 dom2 sop vecs g1[2] g2[2]`                | `found [value u0 v0 u1 v1]` |
//! | `LocalSss`       | `SmSurface::LocalSurfaceSurfaceIntersect` | `s2 s3 dom1 dom2 dom3 g1[2] g2[2] g3[2]`              | `found [value u0 v0 u1 v1 u2 v2]` |
//!
//! The two equation sets are verbatim copies of the file-local upstream classes. [`eval`]
//! accepts only what upstream handles without undefined behaviour or runaway work (as
//! unit `surface.surface_p7`): well-formed clamped B-splines, planes with a non-zero
//! normal, parameters within one domain length of the surface's domain, signed directed
//! solves with a vector, a B-spline surface to cover, at most 32 points per call.

use crate::{call_with_buffer, FfiError};

use super::surface_surface_p7::{
    bad, domain_in, sop, surface_kind, surface_rev, uv, vectors, Domain, Reader, SO_MAXIMIZE,
    SO_MINIMIZE, SO_NORMALIZE, SO_SIGNED_MIN,
};

/// Most points per equation-set call.
pub const MAX_POINTS: usize = 32;

/// Shim operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum Op {
    EvalGeometric,
    NormalSection,
    Singularities,
    Cover,
    ShapeMatch,
    FindPsNormal,
    SssIntersect,
    LocalPlane,
    LocalSolve,
    LocalSss,
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
        pub fn sc_surface_surface_p3_eval(
            op: i32,
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

/// `count` then `count` parameter vectors of `bounds.len()` finite values each, every
/// coordinate within its `[lo, hi]`.
fn points(rd: &mut Reader<'_>, bounds: &[(f64, f64)]) -> Result<(), FfiError> {
    let count = rd.index(MAX_POINTS)?;
    if count == 0 {
        return Err(bad("no points"));
    }
    for _ in 0..count {
        for &(lo, hi) in bounds {
            let x = rd.finite()?;
            if x < lo || x > hi {
                return Err(bad("parameter too far outside its domain"));
            }
        }
    }
    Ok(())
}

/// The bounds of the `(u, v)` parameters of a surface (one domain length around it).
fn uv_bounds(dom: Domain) -> Vec<(f64, f64)> {
    match dom {
        Some([u0, v0, u1, v1]) => vec![
            (u0 - (u1 - u0), u1 + (u1 - u0)),
            (v0 - (v1 - v0), v1 + (v1 - v0)),
        ],
        None => vec![(-1e4, 1e4), (-1e4, 1e4)],
    }
}

/// A parameter pair within one domain length of `dom`.
fn guess(rd: &mut Reader<'_>, dom: Domain) -> Result<(), FfiError> {
    for (lo, hi) in uv_bounds(dom) {
        let x = rd.finite()?;
        if x < lo || x > hi {
            return Err(bad("guess too far outside the domain"));
        }
    }
    Ok(())
}

/// Checks `input` for `op` (see the module docs).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for anything the shim must not see.
pub fn validate(op: Op, input: &[f64]) -> Result<(), FfiError> {
    let mut rd = Reader { input, at: 0 };
    let (dom, is_bspline) = surface_kind(&mut rd, false, true)?;
    match op {
        Op::EvalGeometric => {
            uv(&mut rd, dom)?;
            rd.flag()?;
            rd.flag()?;
        }
        Op::NormalSection => {
            uv(&mut rd, dom)?;
            rd.flag()?;
            rd.flag()?;
            rd.vec3()?;
        }
        Op::Singularities => {
            rd.flag()?;
            rd.flag()?;
        }
        Op::Cover => {
            if !is_bspline {
                return Err(bad("this op needs a B-spline surface"));
            }
            let d2 = surface_rev(&mut rd)?;
            rd.positive("tolerance")?;
            if rd.flag()? {
                domain_in(&mut rd, d2)?;
            } else {
                rd.finites(4)?;
            }
        }
        Op::ShapeMatch => {
            let d2 = surface_rev(&mut rd)?;
            uv(&mut rd, dom)?;
            uv(&mut rd, d2)?;
            rd.positive("tolerance")?;
            rd.positive("angle tolerance")?;
        }
        Op::FindPsNormal => {
            rd.vec3()?;
            rd.positive("squared tolerance")?;
            points(&mut rd, &uv_bounds(dom))?;
        }
        Op::SssIntersect => {
            let d2 = surface_rev(&mut rd)?;
            let d3 = surface_rev(&mut rd)?;
            let mut bounds = uv_bounds(dom);
            bounds.extend(uv_bounds(d2));
            bounds.extend(uv_bounds(d3));
            points(&mut rd, &bounds)?;
        }
        Op::LocalPlane => {
            let d2 = surface_rev(&mut rd)?;
            domain_in(&mut rd, dom)?;
            domain_in(&mut rd, d2)?;
            rd.positive("tolerance")?;
            guess(&mut rd, dom)?;
            guess(&mut rd, d2)?;
            rd.vec3()?;
            rd.vec3()?; // a zero plane normal is fine: Unitize gives (0, 0, 1)
        }
        Op::LocalSolve => {
            let d2 = surface_rev(&mut rd)?;
            domain_in(&mut rd, dom)?;
            domain_in(&mut rd, d2)?;
            let s = sop(
                &mut rd,
                &[SO_MINIMIZE, SO_MAXIMIZE, SO_NORMALIZE, SO_SIGNED_MIN],
            )?;
            let n = vectors(&mut rd)?;
            if s == SO_SIGNED_MIN && n == 0 {
                return Err(bad("signed directed solve needs a vector"));
            }
            guess(&mut rd, dom)?;
            guess(&mut rd, d2)?;
        }
        Op::LocalSss => {
            let d2 = surface_rev(&mut rd)?;
            let d3 = surface_rev(&mut rd)?;
            domain_in(&mut rd, dom)?;
            domain_in(&mut rd, d2)?;
            domain_in(&mut rd, d3)?;
            guess(&mut rd, dom)?;
            guess(&mut rd, d2)?;
            guess(&mut rd, d3)?;
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
            sys::sc_surface_surface_p3_eval(
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

    /// Kind 3 is accepted by p3 (the only shim that builds a surface of revolution).
    #[test]
    fn kind_3_accepted() {
        let probe = [
            3.0, 1.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0,
        ];
        assert!(validate(Op::Singularities, &probe).is_ok());
    }

    /// A generator on the axis makes `CreateCanonical` fail; the shim returns an error
    /// rather than dereferencing a null surface.
    #[test]
    fn generator_on_axis_is_an_error() {
        let probe = [
            3.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0,
        ];
        assert!(validate(Op::Singularities, &probe).is_ok());
        assert!(eval(Op::Singularities, &probe).is_err());
    }
}
