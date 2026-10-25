//! FFI wrappers for unit `blend.fillet_geom_p6`.
//!
//! Shim: `harness/cpp/shim/blend_fillet_geom_p6.cpp` (`sc_blend_fillet_geom_p6_eval`).
//! Upstream: `source/SMLib/src/SmFilletGeom.cpp` (`SmFilletEdge::CalcBlendingRail`,
//! `CalcEdgeOnEdge`, `CalcFilletEnd`, `CalcSetBackRail`, `ShouldSwapFaces`,
//! `SmFilletGeom::Copy`, `FindEdgeTangentAtPoint`, `GetRailIndexOfSideEU`,
//! `GetRailIndexOfVertexuse`, `GetRailVertex`, `SetUpRailPoint`, `RailEdgeuseIntersect`,
//! `RecordFilletError`, and the file-statics `JoinTwoFilletVertsByCurves`, `sm_MoveTSP`).
//!
//! Every op's input starts with a B-rep block of unit `topology.edgeuse_p1` (the target
//! B-rep; `path` is its `.smb` file for a file block), whose objects are addressed by
//! export index. Surface and curve blocks are that unit's. Then the model:
//!
//! - `ns` solvers: `tol ang tang eu0 eu1` (target edge uses, `-1` none), per rail
//!   `has [surface dist]` (an `SmOffsetSurface(dist, base)` installed with `SetSurface`),
//!   `has_exec reverse_trim` (`has_exec`: the solver's executive is the model's
//!   `SmFilletExecutive` on the target);
//! - `nc` corners: `vertex type tol` (the filleted target vertex or `-1`, the
//!   `SmFilletCornerType`, the corner's tolerance);
//! - `nv` fillet vertices: `x y z type status corner pc_class pc_kind pc_index pc_face t u
//!   v` (`corner` a corner index or `-1`; `pc_kind` `0` none, `1` target vertex, `2` target
//!   edge use, `3` target face, `4` target edge `pc_index`; `pc_face` a target face or `-1`);
//! - `ng` geometries: `type status solver has_surface [surface] has_center [curve] dev0
//!   dev1`, per rail `has [surface dist]` (`SetOffsetSurface`);
//! - `ne` edges (`MakeFilletEdge`, in creation order): `vs ve type status corner
//!   has_curve [curve] has_ivl [t0 t1] orig_edge orig_face` (target, `-1` none), then per
//!   edge use (primary, mate) `has_uv [curve] geom u v` and its vertex use's intersection
//!   point `point_type u0 v0 u1 v1 crv[3] deriv[3] curve_parameter`;
//! - per geometry `rail0 rail1 n1 edges.. n2 edges.. nsides (edge which)..`;
//! - per vertex `mate0 mate1`.
//!
//! | [`Op`]              | upstream                                    | arguments after the model |
//! |---------------------|---------------------------------------------|---------------------------|
//! | `Export`            | (the built model)                           |                           |
//! | `BlendingRail`      | `SmFilletEdge::CalcBlendingRail`            | `e use_corner`            |
//! | `EdgeOnEdge`        | `SmFilletEdge::CalcEdgeOnEdge`              | `e use_corner`            |
//! | `FilletEnd`         | `SmFilletEdge::CalcFilletEnd`               | `e use_corner`            |
//! | `SetBackRail`       | `SmFilletEdge::CalcSetBackRail`             | `e use_corner`            |
//! | `ShouldSwap`        | `SmFilletEdge::ShouldSwapFaces`             | `e face1 face2`           |
//! | `Join`              | `JoinTwoFilletVertsByCurves`                | `g vs ve uv_tol n (e which with_uv orient)..` |
//! | `Copy`              | `SmFilletGeom::Copy`                        | `g other`                 |
//! | `FindTangent`       | `SmFilletGeom::FindEdgeTangentAtPoint`      | `g rail end`              |
//! | `RailIndexSideEu`   | `SmFilletGeom::GetRailIndexOfSideEU`        | `g target_eu`             |
//! | `RailIndexVu`       | `SmFilletGeom::GetRailIndexOfVertexuse`     | `g e which`               |
//! | `RailVertex`        | `SmFilletGeom::GetRailVertex`               | `g rail end`              |
//! | `SetUpRailPoint`    | `SmFilletGeom::SetUpRailPoint`              | `g rail end param has_pc pc_class pc_kind pc_index pc_t` |
//! | `MoveTsp`           | `sm_MoveTSP`                                | `n (crv[3] deriv[3]).. from to` |
//! | `RailEuIntersect`   | `SmFilletGeom::RailEdgeuseIntersect`        | `g rail target_eu guess`  |
//! | `RecordError`       | `SmFilletGeom::RecordFilletError`           | `g type`                  |
//!
//! `use_corner` passes the edge's own corner (else `NULL`). The `Join` lists start with
//! the curves of the given edges (the UV entry the edge use's own trim curve when
//! `with_uv`, else `NULL`); `orient` is an `SmOrientType`.
//!
//! Output: `1 status` when the target B-rep cannot be built; else `0`, the op's status,
//! the op's results (`ShouldSwap`: the flag; `Join`: `made n (curve uvcurve orient)..`;
//! `FindTangent`: `found tan[3]`; `RailIndex*`: the index (`99` on an error status);
//! `RailVertex`: the vertex index; `MoveTsp`: `n` and the new order; `RailEuIntersect`:
//! `found param`; `RecordError`: `code status edge_type filleted_edge side_face ne e..
//! ns has_s.. comment_len`), then the export: `nv` then per vertex `x y z type status u v pc_class
//! t`; `ne` then per edge `type status curve t0 t1 orig_edge orig_face` and per edge use
//! `uvcurve geom u v`; `ng` then per geometry `status nsides (edge which).. nrefs (edge
//! which).. surface centre_line dev0 dev1` and per rail `has [dist base_surface]`. A
//! curve is `has dim n p m cpts(x y z w) knots`, a surface `has n m p q r s cpts u v`.
//!
//! Beyond the block checks here, the shim refuses (`SC_ERR_ARG`) a model on which the op
//! would read through `NULL` or a blind cast upstream: missing rails, primary edge uses,
//! mates or rail curves the calculators dereference; classification objects of the
//! wrong kind; geometries without the solver (or executive) an op reads; a `Join`
//! without curves, side edges or the fillet surface it evaluates.

use crate::units::topology_edgeuse_p1::{self as eu1, bad, brep_block, curve, surface, Reader};
use crate::{call_with_buffer, FfiError};

/// Largest number of solvers, corners, vertices, geometries, edges or list entries.
pub const MAX_LIST: usize = 64;

/// The shim's ops.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// The built model.
    Export,
    /// `SmFilletEdge::CalcBlendingRail`.
    BlendingRail,
    /// `SmFilletEdge::CalcEdgeOnEdge`.
    EdgeOnEdge,
    /// `SmFilletEdge::CalcFilletEnd`.
    FilletEnd,
    /// `SmFilletEdge::CalcSetBackRail`.
    SetBackRail,
    /// `SmFilletEdge::ShouldSwapFaces`.
    ShouldSwap,
    /// `JoinTwoFilletVertsByCurves`.
    Join,
    /// `SmFilletGeom::Copy`.
    Copy,
    /// `SmFilletGeom::FindEdgeTangentAtPoint`.
    FindTangent,
    /// `SmFilletGeom::GetRailIndexOfSideEU`.
    RailIndexSideEu,
    /// `SmFilletGeom::GetRailIndexOfVertexuse`.
    RailIndexVu,
    /// `SmFilletGeom::GetRailVertex`.
    RailVertex,
    /// `SmFilletGeom::SetUpRailPoint`.
    SetUpRailPoint,
    /// `sm_MoveTSP`.
    MoveTsp,
    /// `SmFilletGeom::RailEdgeuseIntersect`.
    RailEuIntersect,
    /// `SmFilletGeom::RecordFilletError`.
    RecordError,
}

impl Op {
    /// The shim's op code.
    #[must_use]
    pub fn code(self) -> i32 {
        match self {
            Self::Export => 0,
            Self::BlendingRail => 1,
            Self::EdgeOnEdge => 2,
            Self::FilletEnd => 3,
            Self::SetBackRail => 4,
            Self::ShouldSwap => 5,
            Self::Join => 6,
            Self::Copy => 7,
            Self::FindTangent => 8,
            Self::RailIndexSideEu => 9,
            Self::RailIndexVu => 10,
            Self::RailVertex => 11,
            Self::SetUpRailPoint => 12,
            Self::MoveTsp => 13,
            Self::RailEuIntersect => 14,
            Self::RecordError => 15,
        }
    }
}

mod sys {
    extern "C" {
        pub fn sc_blend_fillet_geom_p6_eval(
            op: i32,
            path: *const std::ffi::c_char,
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

/// An index below `count` or `-1`; `None` for `-1`.
fn opt_below(rd: &mut Reader<'_>, count: usize) -> Result<Option<usize>, FfiError> {
    let v = rd.num()?;
    if v == -1.0 {
        return Ok(None);
    }
    rd.at -= 1;
    rd.below(count).map(Some)
}

/// A curve block of dimension `dim`.
fn curve_of_dim(rd: &mut Reader<'_>, dim: f64, what: &str) -> Result<(), FfiError> {
    if rd.input.get(rd.at).copied() != Some(dim) {
        return Err(bad(format!("{what}: curve of the wrong dimension")));
    }
    curve(rd).map(|_| ())
}

/// Two offset surface slots: `has [surface dist]` each.
fn offsets(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    for _ in 0..2 {
        if rd.flag()? {
            surface(rd)?;
            rd.finite()?;
        }
    }
    Ok(())
}

/// Validates the model after the B-rep block (counts from the B-rep's export).
fn validate_model(rd: &mut Reader<'_>, counts: [usize; 10]) -> Result<[usize; 4], FfiError> {
    let [_, _, _, n_f, _, _, n_eu, n_e, _, n_v] = counts;

    let ns = rd.index(MAX_LIST)?;
    for _ in 0..ns {
        let tols = rd.finites(3)?;
        if tols.iter().take(2).any(|&t| t <= 0.0) {
            return Err(bad("solver: tolerances must be positive"));
        }
        opt_below(rd, n_eu)?;
        opt_below(rd, n_eu)?;
        offsets(rd)?;
        rd.flag()?;
        rd.flag()?;
    }

    let nc = rd.index(MAX_LIST)?;
    for _ in 0..nc {
        opt_below(rd, n_v)?;
        rd.index(12)?; // SmFilletCornerType
        if rd.finite()? <= 0.0 {
            return Err(bad("corner: tolerance must be positive"));
        }
    }

    let nv = rd.index(MAX_LIST)?;
    for _ in 0..nv {
        rd.finites(3)?;
        rd.index(13)?; // SmFilletVertexType
        rd.index(12)?; // SmFilStatus
        opt_below(rd, nc)?; // corner
                            // SmPointClassificationType: SM_PC_UNKNOWN (100) ..= SM_PC_NOTHING (110)
        if rd.index(110)? < 100 {
            return Err(bad("vertex: point class below SM_PC_UNKNOWN"));
        }
        pc_object(rd, [n_v, n_eu, n_f, n_e])?;
        opt_below(rd, n_f)?; // pc_face
        rd.finites(3)?; // t u v
    }

    let ng = rd.index(MAX_LIST)?;
    for _ in 0..ng {
        rd.index(6)?; // SmFilletGeomType
        rd.index(12)?; // SmFilStatus
        opt_below(rd, ns)?;
        if rd.flag()? {
            surface(rd)?;
        }
        if rd.flag()? {
            curve_of_dim(rd, 3.0, "centre line")?;
        }
        rd.finites(2)?;
        offsets(rd)?;
    }

    let ne = rd.index(MAX_LIST)?;
    for _ in 0..ne {
        rd.below(nv)?;
        rd.below(nv)?;
        rd.index(14)?; // SmFilletEdgeType
        rd.index(12)?; // SmFilStatus
        opt_below(rd, nc)?; // corner
        if rd.flag()? {
            curve_of_dim(rd, 3.0, "edge")?;
        }
        if rd.flag()? {
            let t = rd.finites(2)?;
            if t.first() > t.get(1) {
                return Err(bad("edge: inverted interval"));
            }
        }
        opt_below(rd, n_e)?;
        opt_below(rd, n_f)?;
        for _ in 0..2 {
            if rd.flag()? {
                curve_of_dim(rd, 2.0, "uv trim curve")?;
            }
            opt_below(rd, ng)?;
            rd.finites(2)?; // vertex use uv
            rd.index(7)?; // SmIntersectionPointType
            rd.finites(4 + 3 + 3 + 1)?;
        }
    }

    for _ in 0..ng {
        opt_below(rd, ne)?;
        opt_below(rd, ne)?;
        for _ in 0..2 {
            let n = rd.index(MAX_LIST)?;
            for _ in 0..n {
                rd.below(ne)?;
            }
        }
        let n = rd.index(MAX_LIST)?;
        for _ in 0..n {
            rd.below(ne)?;
            rd.index(1)?;
        }
    }

    for _ in 0..nv {
        opt_below(rd, nv)?;
        opt_below(rd, nv)?;
    }
    Ok([ns, nv, ng, ne])
}

/// A classification object `kind index` (`kind` `0` none; `1` .. `4` a target vertex,
/// edge use, face, edge below the given counts).
fn pc_object(rd: &mut Reader<'_>, counts: [usize; 4]) -> Result<(), FfiError> {
    let kind = rd.index(4)?;
    let index = opt_below(rd, MAX_LIST * MAX_LIST * MAX_LIST)?;
    let ok = match (kind, index) {
        (0, _) => true,
        (k, Some(i)) => counts.get(k - 1).is_some_and(|&n| i < n),
        _ => false,
    };
    if ok {
        Ok(())
    } else {
        Err(bad("classification object out of range"))
    }
}

fn raw(op: Op, path: &str, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    let c_path = std::ffi::CString::new(path).map_err(|_| bad("path with NUL"))?;
    call_with_buffer(4096, |buf, len| {
        // SAFETY: c_path is a valid NUL-terminated string and input is valid for
        // input.len() reads, holding what the shim reads for `op` (validated by eval:
        // well-formed blocks, indices inside the B-rep's export and the model's lists; a
        // B-rep block alone when the B-rep cannot be built, after which the shim reads
        // nothing more); buf is valid for buf.len() writes and the shim honours the
        // capacity; len is valid.
        unsafe {
            sys::sc_blend_fillet_geom_p6_eval(
                op.code(),
                c_path.as_ptr(),
                input.as_ptr(),
                input.len() as i64,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}

/// Runs `op` on `input` (layouts in the module docs; `path` is the `.smb` file of a file
/// B-rep block, else empty).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for input the shim must not see; [`FfiError::Status`]
/// for a shim failure (`SC_ERR_ARG` for a model the shim refuses, see the module docs).
pub fn eval(op: Op, path: &str, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    let mut rd = Reader { input, at: 0 };
    brep_block(&mut rd, path)?;
    let block = input.get(..rd.at).ok_or_else(|| bad("input too short"))?;
    let export = eu1::eval(eu1::Op::Export, path, block)?;
    let Some(counts) = eu1::counts(&export) else {
        // the B-rep cannot be built: the shim reports the same failure
        return raw(op, path, block);
    };
    let [_, _, _, n_f, _, _, n_eu, n_e, _, n_v] = counts;
    let [_, nv, ng, ne] = validate_model(&mut rd, counts)?;
    match op {
        Op::Export => {}
        Op::BlendingRail | Op::EdgeOnEdge | Op::FilletEnd | Op::SetBackRail => {
            rd.below(ne)?;
            rd.flag()?;
        }
        Op::ShouldSwap => {
            rd.below(ne)?;
            rd.below(n_f)?;
            rd.below(n_f)?;
        }
        Op::Join => {
            rd.below(ng)?;
            opt_below(&mut rd, nv)?;
            opt_below(&mut rd, nv)?;
            rd.finite()?;
            let n = rd.index(MAX_LIST)?;
            for _ in 0..n {
                rd.below(ne)?;
                rd.index(1)?;
                rd.flag()?;
                rd.index(2)?; // SmOrientType
            }
        }
        Op::Copy => {
            rd.below(ng)?;
            rd.below(ng)?;
        }
        Op::FindTangent | Op::RailVertex => {
            rd.below(ng)?;
            rd.index(1)?;
            rd.index(1)?;
        }
        Op::RailIndexSideEu => {
            rd.below(ng)?;
            rd.below(n_eu)?;
        }
        Op::RailIndexVu => {
            rd.below(ng)?;
            rd.below(ne)?;
            rd.index(1)?;
        }
        Op::SetUpRailPoint => {
            rd.below(ng)?;
            rd.index(1)?;
            rd.index(1)?;
            rd.finite()?;
            rd.flag()?;
            if rd.index(110)? < 100 {
                return Err(bad("point class below SM_PC_UNKNOWN"));
            }
            pc_object(&mut rd, [n_v, n_eu, n_f, n_e])?;
            rd.finite()?;
        }
        Op::MoveTsp => {
            let n = rd.index(MAX_LIST)?;
            for _ in 0..n {
                rd.finites(6)?;
            }
            rd.index(MAX_LIST)?;
            rd.index(MAX_LIST)?;
        }
        Op::RailEuIntersect => {
            rd.below(ng)?;
            rd.index(1)?;
            rd.below(n_eu)?;
            rd.finite()?;
        }
        Op::RecordError => {
            rd.below(ng)?;
            rd.index(7)?; // SmFilletErrorType
        }
    }
    rd.done()?;
    raw(op, path, input)
}
