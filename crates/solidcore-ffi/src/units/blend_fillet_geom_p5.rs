//! FFI wrappers for unit `blend.fillet_geom_p5`.
//!
//! Shim: `harness/cpp/shim/blend_fillet_geom_p5_static.inc` (`sc_blend_fillet_geom_p5_eval`),
//! compiled inside `blend_fillet_geom_p1.cpp`, the one translation unit that compiles
//! upstream `SmFilletGeom.cpp` (every op builds `blend.fillet_geom_p1`'s pseudo-B-rep
//! model and `blend.fillet_geom_p4`'s base solvers there); `blend_fillet_geom_p5.cpp` only
//! documents that. Upstream: `source/SMLib/src/SmFilletGeom.cpp`.
//!
//! Every op starts with `blend.fillet_geom_p1`'s pseudo-B-rep block (layout in
//! [`super::blend_fillet_geom_p1`]), then `blend.fillet_geom_p4`'s solvers (`ns`, per
//! solver `approx ang tang`, then per geometry its solver index or `-1`). Then the op's
//! parameters:
//!
//! | [`Op`]        | upstream                                   | parameters | outputs (after the status) |
//! |---------------|--------------------------------------------|------------|----------------------------|
//! | `IntFillet`   | `SmFilletVertex::CalcFilletIntFillet`      | `v`        | `pnt[3] status` of the vertex |
//! | `CrossSection`| `SmFilletEdge::CalcCrossSection(ctx, NULL)` | `e`       | edge curve, primary and mate UV curves (`has ...`), per geometry its side-use count |
//! | `SideFace`    | `SmFilletEdge::CalcFilletIntSideFace(ctx, NULL)` | `e`  | edge status, per geometry its side-use count |
//! | `ExtEdge`     | `SmFilletEdge::CalcEdgeOnExtendedEdge(ctx, NULL)` | `e` | (none) |
//! | `TrimRails`   | `SmFilletGeom::TrimRailCurves`             | `g`        | per rail (the two rails, then the other rails of side 1, then of side 2): `has`, then its curve, interval, primary and mate UV curves, both vertex uses' UV points |
//! | `SplitFace`   | `SmFilletGeom::UpdateSplitFace(f1, f2)` (the edges' original faces are set first) | `g f1 f2` then per edge (edge-list order) its original face or `-1` | per edge: its original face index (`-1` none), the split-face count and the indices |
//! | `MakeEdge`    | `SmFilletEdge::MakeFilletEdge(pseudo, vs, ve, e, NULL)` | `vs ve` (`-1`: NULL) | the edge count; after a success the pseudo-B-rep export of [`super::blend_fillet_geom_p1`] (the new edge's type, both uses' orientations and list order, each vertex use's vertex, the vertex use lists) |
//! | `RailEdge`    | `SmFilletGeom::MakeRailEdge(pseudo, rail, e)` | `g rail` | the edge count; after a success the export (the geometry's rail and vertices, with the two new vertices appended to the model's vertex list) |
//! | `EndCurve`    | `SmFilletGeom::CreateEndCurve(end, trim, pseudo)` | `g end trim` | the edge count; after a success the export (the new edge's 3D curve, interval and UV curves, the geometry's side uses) |
//! | `RailEdgeuse` | `SmFilletGeom::MakeRailEdgeuse(pseudo, rail, fv, eu, orient, belonging)` | `g rail fv orient belonging` (`-1`: NULL; `orient` is `SmOrientType`) | the edge count; after a success the export. The base solvers carry no edge uses, so every run past the allocations ends in `NER(pBaseEU)`; only the two checks before them (`NER(pFilletSolver)`, `NER(pFilletVertex)`) are compared until `topology.brep_p3` |
//!
//! Refusals (`SC_ERR_ARG`, decided by the shim on the built model; each is an input that is UB
//! or a crash upstream, not one C++ handles):
//! - a fillet corner that would be dereferenced (corners are opaque tags), a solver, rail or
//!   curve upstream dereferences without a check;
//! - a `CrossSection` whose `sm_CreateUVCurve` succeeds without any curve (upstream then lifts
//!   the missing UV curve, UB-213), and a `SideFace` past `pCorner->IsExtendedSurfacePatch()`
//!   with the `NULL` corner (UB-214);
//! - `TrimRails` of a cliff rollover: an unprocessed `SM_FV_CLIFF_RAIL_X_EDGE` vertex whose
//!   classification object is not an `SmEdgeuse` (`CalcCliffRailIntEdge` casts it), and a cliff
//!   rail whose edge uses name no other geometry (its side surface would come from the rail's
//!   original face, which the model does not have);
//! - `SplitFace`: an edge whose original face is `f1` and whose `ShouldSwapFaces` would
//!   dereference a missing curve, a corner, or an uninitialised test point;
//! - `EndCurve` with `SM_BT_BLEND`: a rail end vertex classified `SM_PC_EDGE` whose object is
//!   not an `SmEdge` (`FindEdgeTangentAtPoint` casts it; a `NULL` object is a defined error and
//!   runs);
//! - `RailEdgeuse` past `NER(pFilletSolver)` / `NER(pFilletVertex)` with a solver that lacks
//!   one of its two base edge uses (UB-238: the clean-up deletes a vertex use still linked in
//!   the fillet vertex, and `pBaseEU1->GetFace()` dereferences `NULL`).

use crate::{call_with_buffer, FfiError};

use super::blend_fillet_geom_p1::{validate_block, Reader, MAX_LIST, MAX_SOLVER_MAGNITUDE};

/// The shim's ops.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum Op {
    IntFillet = 0,
    CrossSection = 1,
    SideFace = 2,
    ExtEdge = 3,
    TrimRails = 4,
    SplitFace = 5,
    MakeEdge = 6,
    RailEdge = 7,
    EndCurve = 8,
    RailEdgeuse = 9,
}

mod sys {
    extern "C" {
        pub fn sc_blend_fillet_geom_p5_eval(
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

/// Checks an input program: the model block, the solvers and the op's parameters.
/// Magnitudes are bounded as for `blend.fillet_geom_p4`'s `UvCurve` (the surface point
/// solves of `sm_CreateUVCurve`, the curve drops and solves).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for a malformed program.
pub fn validate(op: Op, input: &[f64]) -> Result<(), FfiError> {
    let mut rd = Reader {
        cursor: crate::reader::Cursor::new(input),
        bound: MAX_SOLVER_MAGNITUDE,
    };
    let b = validate_block(&mut rd)?;

    let ns = rd.index(MAX_LIST)?;
    for _ in 0..ns {
        rd.finites(3)?;
    }
    for _ in 0..b.ng() {
        rd.opt_index(ns)?;
    }

    match op {
        Op::IntFillet => {
            rd.opt_index(b.nv())?.ok_or_else(|| bad("vertex -1"))?;
        }
        Op::CrossSection | Op::SideFace | Op::ExtEdge => {
            rd.opt_index(b.ne())?.ok_or_else(|| bad("edge -1"))?;
        }
        Op::TrimRails => {
            rd.opt_index(b.ng())?.ok_or_else(|| bad("geometry -1"))?;
        }
        Op::SplitFace => {
            rd.opt_index(b.ng())?.ok_or_else(|| bad("geometry -1"))?;
            rd.opt_index(b.nf)?.ok_or_else(|| bad("face -1"))?;
            rd.opt_index(b.nf)?.ok_or_else(|| bad("face -1"))?;
            for _ in 0..b.ne() {
                rd.opt_index(b.nf)?;
            }
        }
        Op::MakeEdge => {
            rd.opt_index(b.nv())?;
            rd.opt_index(b.nv())?;
        }
        Op::RailEdge => {
            rd.opt_index(b.ng())?.ok_or_else(|| bad("geometry -1"))?;
            rd.index(2)?;
        }
        Op::EndCurve => {
            rd.opt_index(b.ng())?.ok_or_else(|| bad("geometry -1"))?;
            rd.index(2)?;
            rd.index(5)?;
        }
        Op::RailEdgeuse => {
            rd.opt_index(b.ng())?.ok_or_else(|| bad("geometry -1"))?;
            rd.index(2)?;
            rd.opt_index(b.nv())?;
            rd.index(3)?;
            rd.opt_index(b.ne())?;
        }
    }
    rd.done()
}

/// Runs one op.
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for an input [`validate`] or the shim refuses; the
/// other [`FfiError`]s of the call.
pub fn eval(op: Op, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    validate(op, input)?;
    call_with_buffer(4096, |buf, len| {
        // SAFETY: `input` is valid for `input.len()` reads, `buf` for `buf.len()`
        // writes and `len` is a valid out-pointer; the shim checks every count.
        unsafe {
            sys::sc_blend_fillet_geom_p5_eval(
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
