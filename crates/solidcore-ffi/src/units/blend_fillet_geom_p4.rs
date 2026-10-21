//! FFI wrappers for unit `blend.fillet_geom_p4`.
//!
//! Shim: `harness/cpp/shim/blend_fillet_geom_p4_static.inc` (`sc_blend_fillet_geom_p4_eval`),
//! compiled inside `blend_fillet_geom_p1.cpp`, the one translation unit that compiles
//! upstream `SmFilletGeom.cpp` (the unit's routines include the file-static
//! `sm_CreateUVCurve` and `sm_CalcNx2VertFilletXFillet`); `blend_fillet_geom_p4.cpp` only
//! documents that. Upstream: `source/SMLib/src/SmFilletGeom.cpp`.
//!
//! Every op starts with `blend.fillet_geom_p1`'s pseudo-B-rep block (layout in
//! [`super::blend_fillet_geom_p1`]; the shim builds the same `SmFilletBrep` model), then
//! the solvers: `ns`, per solver `approx ang tang` (a base `SmFilletSolver` with those
//! tolerances), then per geometry its solver index or `-1` (the geometry's
//! `m_pFilletSolver`). Then the op's parameters:
//!
//! | [`Op`]             | upstream                                     | parameters          | outputs (after the status) |
//! |--------------------|----------------------------------------------|---------------------|---------------------------|
//! | `UvCurve`          | `sm_CreateUVCurve`                           | `sv ev two kind [surface] tol` | the 3D curve, UV curve 1, UV curve 2 (`has ...`) |
//! | `RailIndex`        | `SmFilletGeom::GetRailIndexOfVertex`         | `g v` (`-1`: NULL)  | `idx` |
//! | `RailIntExtended`  | `SmFilletVertex::CalcRailIntExtendedEdgeuse` | `v`                 | `pnt[3] status` of the vertex |
//! | `Int2Fillets`      | `SmFilletVertex::CalcFilletInt2Fillets`      | `v`                 | `pnt[3] status` of the vertex |
//! | `Nx2`              | `sm_CalcNx2VertFilletXFillet`                | `v`                 | `pnt[3] status` of the vertex |
//! | `MakeFace`         | `SmFilletGeom::MakeFaceBrep` (into a new `SmBrep`) | `g`           | (none) |
//! | `Blending`         | `SmFilletGeom::CalcBlendingGeom`             | `g`                 | (none) |
//!
//! The `UvCurve` surface `kind` is `0` a B-spline surface (layout of
//! [`super::blend_fillet_geom_p1`]), `1` a real `SmPlane(O, X, Y, UVScale, UVDomain)`
//! (`O[3] X[3] Y[3] scale[2] umin vmin umax vmax make_nurb`), `2` a real
//! `SmSurfOfRevolution` (`CreateCanonical` of the line `a b` about an axis: `a[3] b[3]
//! axis_point[3] axis_dir[3] swap`).
//!
//! [`eval`] accepts what `blend.fillet_geom_p1`'s block validation accepts plus
//! well-formed solver and op arguments; `UvCurve` and `MakeFace` values are bounded by
//! [`MAX_SOLVER_MAGNITUDE`](super::blend_fillet_geom_p1::MAX_SOLVER_MAGNITUDE) (their
//! global solves never finish once box arithmetic overflows, HANG-20). The shim refuses
//! (`SC_ERR_ARG`) exactly the inputs that make upstream dereference what the harness
//! cannot provide: a fillet corner (the block's corners are opaque tags, never real
//! `SmFilletCorner` objects) where upstream calls into it (`RailIntExtended` and
//! `Int2Fillets` of a vertex with a corner; `Nx2` once its edge loop finishes, where it
//! asks the corner for its context), a geometry without a solver where upstream
//! dereferences it (`Nx2`'s cross-section edges, `MakeFace` past its rail checks,
//! `Blending`), a missing rail where upstream dereferences it (`RailIndex` of a vertex,
//! `MakeFace`, `Blending`'s first rail), and a `MakeFace` whose
//! `JoinTwoFilletVertsByCurves` would dereference a missing rail or side curve. The
//! block's corners and solvers are never real enough for upstream to get further on
//! those paths.
//!
//! Known gap, unreachable today: `RailIntExtended` of a vertex *without* a corner
//! dereferences `NULL` `m_cpCorner` (`GetExtendedSurface`) as soon as the fillet edge
//! has an original (side) face. The `blend.fillet_geom_p1` model cannot express an
//! original face on a fillet edge, so upstream always stops earlier at
//! `NER(pSideFace)`; once original faces become expressible this refusal must be
//! added.

use crate::{call_with_buffer, FfiError};

use super::blend_fillet_geom_p1::{
    surface, validate_block, Reader, MAX_LIST, MAX_MAGNITUDE, MAX_SOLVER_MAGNITUDE,
};

/// The upstream routine run by the shim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum Op {
    UvCurve = 0,
    RailIndex = 1,
    RailIntExtended = 2,
    Int2Fillets = 3,
    Nx2 = 4,
    MakeFace = 5,
    Blending = 6,
}

mod sys {
    extern "C" {
        pub fn sc_blend_fillet_geom_p4_eval(
            op: i32,
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

fn bad(msg: impl Into<String>) -> FfiError {
    FfiError::InvalidArgument(msg.into())
}

/// Validates `input` for `op` (layouts in the module docs).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for an input upstream could not handle safely.
pub fn validate(op: Op, input: &[f64]) -> Result<(), FfiError> {
    let mut rd = Reader {
        cursor: crate::reader::Cursor::new(input),
        bound: match op {
            Op::UvCurve => MAX_SOLVER_MAGNITUDE,
            Op::MakeFace => MAX_MAGNITUDE,
            _ => f64::INFINITY,
        },
    };
    let b = validate_block(&mut rd)?;

    // solvers
    let ns = rd.index(MAX_LIST)?;
    for _ in 0..ns {
        rd.finites(3)?;
    }
    for _ in 0..b.ng() {
        rd.opt_index(ns)?;
    }

    match op {
        Op::UvCurve => {
            rd.opt_index(b.nv())?
                .ok_or_else(|| bad("start vertex -1"))?;
            rd.opt_index(b.nv())?.ok_or_else(|| bad("end vertex -1"))?;
            rd.flag()?;
            match rd.index(2)? {
                0 => surface(&mut rd)?,
                1 => {
                    // plane: origin, axes, scales, domain, then make_nurb
                    rd.finites(15)?;
                    rd.flag()?;
                }
                _ => {
                    // surface of revolution: line ends, axis point and direction, swap
                    rd.finites(12)?;
                    rd.flag()?;
                }
            }
            rd.finite()?;
        }
        Op::RailIndex => {
            rd.opt_index(b.ng())?.ok_or_else(|| bad("geometry -1"))?;
            rd.opt_index(b.nv())?;
        }
        Op::RailIntExtended | Op::Int2Fillets | Op::Nx2 => {
            rd.opt_index(b.nv())?.ok_or_else(|| bad("vertex -1"))?;
        }
        Op::MakeFace | Op::Blending => {
            rd.opt_index(b.ng())?.ok_or_else(|| bad("geometry -1"))?;
        }
    }
    rd.done()
}

/// Runs `op` on the flat `input` (layouts above) and returns the shim's record: the
/// status (`0` = `SM_SUCCESS`, else the `SmStatus`) and the op's outputs.
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for an input upstream could not handle safely;
/// [`FfiError::Status`] for a shim error (`SC_ERR_*`, `SC_ERR_ARG` for the refusals in
/// the module docs).
pub fn eval(op: Op, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    validate(op, input)?;
    call_with_buffer(4096, |buf, len| {
        // SAFETY: input is valid for input.len() reads (validated above); buf is valid
        // for buf.len() writes; len is a valid pointer.
        unsafe {
            sys::sc_blend_fillet_geom_p4_eval(
                op as i32,
                input.as_ptr(),
                input.len() as i64,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}
