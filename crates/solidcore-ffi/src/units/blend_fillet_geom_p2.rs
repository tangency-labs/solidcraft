//! FFI wrappers for unit `blend.fillet_geom_p2`.
//!
//! Shim: `harness/cpp/shim/blend_fillet_geom_p2_static.inc` (`sc_blend_fillet_geom_p2_eval`),
//! compiled inside `blend_fillet_geom_p1.cpp`, the one translation unit that compiles
//! upstream `SmFilletGeom.cpp` (which defines `sm_SrfSrfIntersection`);
//! `blend_fillet_geom_p2.cpp` only documents that. Upstream:
//! `source/SMLib/src/SmFilletGeom.cpp`.
//!
//! A surface is `kind [data]`: `0` a B-spline surface (layout of
//! [`super::blend_fillet_geom_p1`]), `1` a real `SmPlane(O, X, Y, UVScale, UVDomain)`
//! (`O[3] X[3] Y[3] scale[2] umin vmin umax vmax make_nurb`), `2` a real
//! `SmSurfOfRevolution` (`CreateCanonical` of the line `a b` about an axis: `a[3] b[3]
//! axis_point[3] axis_dir[3] swap`), as in [`super::blend_fillet_geom_p4`]. A curve
//! record is `type has [dim n p m cpts knots]` (`type` the curve's `SM_TYPE`, `0` for
//! none; the B-spline data of a kind derived from `SmBSplineCurve`).
//!
//! | [`Op`]           | upstream                                  | parameters | outputs |
//! |------------------|-------------------------------------------|------------|---------|
//! | `SrfSrf`         | `sm_SrfSrfIntersection`                   | `surface1 surface2 has_start start[3] has_end end[3] has_dir dir[3] has_end_dir end_dir[3] tol ang skip has_ref ref[3]` | `cs1 cs2` (construction statuses; nothing else unless both are `0`), status, the 3D curve, UV curve 1, UV curve 2 (a failure's are reported as none), `dir[3]` as left by the call |
//! | `RailIntRail`    | `SmFilletVertex::CalcRailIntRail`         | block, solvers, `v` | status, `pnt[3] status` of the vertex |
//! | `RailIntEdgeuse` | `SmFilletVertex::CalcRailIntEdgeuse`      | block, solvers, `v g` (`-1`: `NULL`) | as above |
//! | `CrossSection`   | `SmFilletVertex::CalcVertOnCrossSection`  | block, solvers, `v has_param param` | as above |
//! | `SetBack`        | `SmFilletVertex::CalcSetBackVert`         | block, solvers, `v` | as above |
//! | `RailRail`       | `SmFilletEdge::CalcRailRailInterpolation` | block, solvers, `e face [tol] nc [edges..]` (`face` `-1`: no original face, else a surface as above and the face tolerance (`SetTolerance(SmZoneTol3d(tol), FALSE, FALSE)`), made a target face and the edge's original face; `nc` `-1`: `pCorner` `NULL`, else a corner of type `SM_FCR_UNKNOWN` whose fillet edges are those `nc` model edges) | `cs`, then (when `0`) status, the edge's fillet status, its curve, the primary and mate edge uses' UV curves |
//! | `SplitFace`      | `SmFilletEdge::UpdateSplitFace`           | block, solvers, `e set f1 f2` (target faces, `-1`: `NULL`; `set` becomes the original face first) | status, original face index, `n` split face indices |
//!
//! The block is [`super::blend_fillet_geom_p1`]'s pseudo-B-rep block, the solvers
//! [`super::blend_fillet_geom_p4`]'s (`ns`, per solver `approx ang tang`, per geometry
//! its solver or `-1`).
//!
//! [`eval`] accepts what the block validation and the surface/curve validation of
//! `blend.fillet_geom_p1` accept plus well-formed op arguments; every value is bounded
//! by [`MAX_SOLVER_MAGNITUDE`](super::blend_fillet_geom_p1::MAX_SOLVER_MAGNITUDE) (the
//! global solvers never finish once box arithmetic overflows, HANG-20). The shim
//! refuses (`SC_ERR_ARG`) exactly the inputs that make upstream dereference what the
//! harness cannot provide or what is `NULL`: a vertex with a fillet corner for the four
//! vertex calculators (the block's corners are opaque tags; each routine asks the
//! corner for its filleted vertex right after its null check), a `RailRail` edge whose
//! chosen rail curve (the mate edge use's UV curve or the rail's 3D curve, picked as
//! upstream picks it) is `NULL` (dropped onto unchecked; not checked when the corner's
//! edge list returns first), a `RailRail` corner of at most four edges whose test edge
//! (two places after the queried one) is a cross-section edge with a `NULL` primary edge use
//! or with a geometry (its solver and surface generator are dereferenced unchecked; the
//! harness's solvers have no generator), a `SplitFace` whose edge's
//! original face is the split face (`ShouldSwapFaces` then dereferences the edge's
//! curve, curve classification or corner and both faces unchecked; the block's
//! corners carry no data, and part 6's own difftest compares `ShouldSwapFaces`), and,
//! as in
//! `blend.fillet_geom_p1`, surfaces whose constructed v knot vector would make the
//! surface cache write out of bounds (UB-166).

use crate::{call_with_buffer, FfiError};

use super::blend_fillet_geom_p1::{
    surface, validate_block, Reader, MAX_LIST, MAX_SOLVER_MAGNITUDE,
};

/// The upstream routine run by the shim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum Op {
    SrfSrf = 0,
    RailIntRail = 1,
    RailIntEdgeuse = 2,
    CrossSection = 3,
    SetBack = 4,
    RailRail = 5,
    SplitFace = 6,
}

mod sys {
    extern "C" {
        pub fn sc_blend_fillet_geom_p2_eval(
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

/// A surface `kind [data]` (module docs).
fn any_surface(rd: &mut Reader<'_>, kind: usize) -> Result<(), FfiError> {
    match kind {
        0 => surface(rd)?,
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
    Ok(())
}

/// Validates `input` for `op` (layouts in the module docs).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for an input upstream could not handle safely.
pub fn validate(op: Op, input: &[f64]) -> Result<(), FfiError> {
    let mut rd = Reader {
        cursor: crate::reader::Cursor::new(input),
        bound: MAX_SOLVER_MAGNITUDE,
    };
    if op == Op::SrfSrf {
        for _ in 0..2 {
            let kind = rd.index(2)?;
            any_surface(&mut rd, kind)?;
        }
        for _ in 0..4 {
            rd.flag()?;
            rd.finites(3)?;
        }
        rd.finites(2)?;
        rd.flag()?;
        rd.flag()?;
        rd.finites(3)?;
        return rd.done();
    }

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
        Op::SrfSrf => {}
        Op::RailIntRail | Op::SetBack => {
            rd.opt_index(b.nv())?.ok_or_else(|| bad("vertex -1"))?;
        }
        Op::RailIntEdgeuse => {
            rd.opt_index(b.nv())?.ok_or_else(|| bad("vertex -1"))?;
            rd.opt_index(b.ng())?;
        }
        Op::CrossSection => {
            rd.opt_index(b.nv())?.ok_or_else(|| bad("vertex -1"))?;
            rd.flag()?;
            rd.finite()?;
        }
        Op::RailRail => {
            rd.opt_index(b.ne())?.ok_or_else(|| bad("edge -1"))?;
            if let Some(kind) = rd.opt_index(3)? {
                any_surface(&mut rd, kind)?;
                rd.finite()?;
            }
            if let Some(n) = rd.opt_index(MAX_LIST)? {
                for _ in 0..n {
                    rd.opt_index(b.ne())?
                        .ok_or_else(|| bad("corner edge -1"))?;
                }
            }
        }
        Op::SplitFace => {
            rd.opt_index(b.ne())?.ok_or_else(|| bad("edge -1"))?;
            for _ in 0..3 {
                rd.opt_index(b.nf)?;
            }
        }
    }
    rd.done()
}

/// Runs `op` on the flat `input` (layouts above) and returns the shim's record.
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
            sys::sc_blend_fillet_geom_p2_eval(
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
