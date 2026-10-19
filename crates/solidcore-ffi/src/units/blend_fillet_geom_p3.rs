//! FFI wrappers for unit `blend.fillet_geom_p3`.
//!
//! Shim: `harness/cpp/shim/blend_fillet_geom_p3.cpp` (`sc_blend_fillet_geom_p3_eval`).
//! Upstream: `source/SMLib/src/SmFilletGeom.cpp` (`SmFilletVertex::CalcCornerVertGeom`,
//! `CalcMateGeom`, `SmFilletEdge::CalcCornerEdgeGeom`, `CalcFilletIntFillet`,
//! `SmFilletGeom::ReCalcFilletGeom`).
//!
//! Every op's input starts with a B-rep block of unit `topology.edgeuse_p1` (the target
//! B-rep; `path` is its `.smb` file for a file block), whose objects are addressed by
//! export index. Surface and curve blocks are that unit's. Then the model:
//!
//! - `ns` solvers: `tol ang tang eu0 eu1` (target edge uses, `-1` none), per rail
//!   `has [surface dist]` (an `SmOffsetSurface(dist, base)` installed with `SetSurface`),
//!   `extend_before extend_after setup` (`setup`: `SetupOffsetValues` succeeds);
//! - `nv` fillet vertices: `x y z type status corner pc_class pc_kind pc_index pc_face t u
//!   v` (`pc_kind` `0` none, `1` target vertex, `2` target edge use, `3` target face
//!   `pc_index`; `pc_face` a target face or `-1`; `corner` an opaque tag or `-1`);
//! - `ng` geometries: `type status solver has_surface [surface]` (`SmFilletGeom(solver,
//!   type)`, `solver` `-1` for none);
//! - `ne` edges (`MakeFilletEdge`, in creation order): `vs ve type status corner
//!   has_curve [curve] has_ivl [t0 t1]`, then per edge use (primary, mate) `has_uv
//!   [curve] geom u v` and its vertex use's intersection point `point_type u0 v0 u1 v1
//!   crv[3] deriv[3] curve_parameter`;
//! - per geometry `rail0 rail1 n1 edges.. n2 edges.. nsides (edge which)..`;
//! - per vertex `mate0 mate1`.
//!
//! | [`Op`]           | upstream                               | arguments after the model |
//! |------------------|----------------------------------------|---------------------------|
//! | `Export`         | (the built model)                      |                           |
//! | `CornerVert`     | `SmFilletVertex::CalcCornerVertGeom`   | `v`                       |
//! | `Mate`           | `SmFilletVertex::CalcMateGeom`         | `v`                       |
//! | `CornerEdge`     | `SmFilletEdge::CalcCornerEdgeGeom`     | `e`                       |
//! | `FilletIntFillet`| `SmFilletEdge::CalcFilletIntFillet(ctx, corner)` | `e`             |
//! | `ReCalc`         | `SmFilletGeom::ReCalcFilletGeom`       | `g analytic has_dir dir[3] has_ref [curve]` |
//!
//! Output: `1 status` when the target B-rep cannot be built; else `0`, the op's status
//! and the export: `nv` then per vertex `x y z type status u v` (original UV); `ne` then
//! per edge `type status curve t0 t1` and per edge use `uvcurve geom point_type u0 v0 u1
//! v1 crv[3] curve_parameter`; `ng` then per geometry `status nsides (edge which).. nrefs
//! (edge which)..`. A curve is `has dim n p m cpts(x y z w) knots`.
//!
//! Beyond the block checks here, the shim refuses (`SC_ERR_ARG`) a model on which the op
//! would read through `NULL` or a blind cast upstream, or run a vertex or edge calculator
//! of another unit (parts 2, 4, 5 and 6 read corner and executive data the shim does not
//! build; the fillet/fillet vertex and surface/surface intersections of
//! `CalcFilletIntFillet` are allowed): mates recursing through a cycle,
//! an `SM_FV_ON_VERTEX` vertex not classified to a target vertex, a mate's main vertex classified
//! to a non-edge-use or the mate to a non-face, a solver or side edge use that
//! `FindIndexOfRailXSideEdgeuse` would dereference while `NULL`, a missing main rail-end
//! vertex use, a fillet/fillet edge whose geometries lack solvers or (on the trimming
//! path) UV trim curves, and a `ReCalc` geometry without a solver, offset surfaces or
//! (not a corner fillet) first solver edge use.

use crate::units::topology_edgeuse_p1::{self as eu1, bad, brep_block, curve, surface, Reader};
use crate::{call_with_buffer, FfiError};

/// Largest number of solvers, vertices, geometries, edges or list entries.
pub const MAX_LIST: usize = 64;

/// The shim's ops.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// The built model.
    Export,
    /// `SmFilletVertex::CalcCornerVertGeom`.
    CornerVert,
    /// `SmFilletVertex::CalcMateGeom`.
    Mate,
    /// `SmFilletEdge::CalcCornerEdgeGeom`.
    CornerEdge,
    /// `SmFilletEdge::CalcFilletIntFillet`.
    FilletIntFillet,
    /// `SmFilletGeom::ReCalcFilletGeom`.
    ReCalc,
}

impl Op {
    /// The shim's op code.
    #[must_use]
    pub fn code(self) -> i32 {
        match self {
            Self::Export => 0,
            Self::CornerVert => 1,
            Self::Mate => 2,
            Self::CornerEdge => 3,
            Self::FilletIntFillet => 4,
            Self::ReCalc => 5,
        }
    }
}

mod sys {
    extern "C" {
        pub fn sc_blend_fillet_geom_p3_eval(
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

/// Validates the model after the B-rep block (counts from the B-rep's export).
fn validate_model(rd: &mut Reader<'_>, counts: [usize; 10]) -> Result<[usize; 4], FfiError> {
    let [_, _, _, n_f, _, _, n_eu, _, _, n_v] = counts;

    let ns = rd.index(MAX_LIST)?;
    for _ in 0..ns {
        // any finite tolerances: zero and negative ones run through every op upstream
        // (the fillet x fillet drop, the mate and the intersector of ReCalcFilletGeom)
        rd.finites(3)?;
        opt_below(rd, n_eu)?;
        opt_below(rd, n_eu)?;
        for _ in 0..2 {
            if rd.flag()? {
                surface(rd)?;
                rd.finite()?;
            }
        }
        rd.flag()?;
        rd.flag()?;
        rd.flag()?;
    }

    let nv = rd.index(MAX_LIST)?;
    for _ in 0..nv {
        rd.finites(3)?;
        rd.index(13)?; // SmFilletVertexType
        rd.index(12)?; // SmFilStatus
        opt_below(rd, MAX_LIST)?; // corner tag

        // SmPointClassificationType: SM_PC_UNKNOWN (100) ..= SM_PC_NOTHING (110)
        if rd.index(110)? < 100 {
            return Err(bad("vertex: point class below SM_PC_UNKNOWN"));
        }
        let pc_kind = rd.index(3)?;
        let pc_index = opt_below(rd, MAX_LIST * MAX_LIST)?;
        let in_range = match (pc_kind, pc_index) {
            (0, _) => true,
            (1, Some(i)) => i < n_v,
            (2, Some(i)) => i < n_eu,
            (3, Some(i)) => i < n_f,
            _ => false,
        };
        if !in_range {
            return Err(bad("vertex: classification object out of range"));
        }
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
    }

    let ne = rd.index(MAX_LIST)?;
    for _ in 0..ne {
        rd.below(nv)?;
        rd.below(nv)?;
        rd.index(14)?; // SmFilletEdgeType
        rd.index(12)?; // SmFilStatus
        opt_below(rd, MAX_LIST)?; // corner tag
        if rd.flag()? {
            curve_of_dim(rd, 3.0, "edge")?;
        }
        if rd.flag()? {
            let t = rd.finites(2)?;
            if t.first() > t.get(1) {
                return Err(bad("edge: inverted interval"));
            }
        }
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
            sys::sc_blend_fillet_geom_p3_eval(
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
    let [_, nv, ng, ne] = validate_model(&mut rd, counts)?;
    match op {
        Op::Export => {}
        Op::CornerVert | Op::Mate => {
            rd.below(nv)?;
        }
        Op::CornerEdge | Op::FilletIntFillet => {
            rd.below(ne)?;
        }
        Op::ReCalc => {
            rd.below(ng)?;
            rd.flag()?;
            rd.flag()?;
            rd.vec3()?;
            if rd.flag()? {
                curve_of_dim(&mut rd, 3.0, "reference curve")?;
            }
        }
    }
    rd.done()?;
    raw(op, path, input)
}
