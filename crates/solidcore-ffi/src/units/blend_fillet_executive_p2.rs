//! FFI wrappers for unit `blend.fillet_executive_p2`.
//!
//! Shim: `harness/cpp/shim/blend_fillet_executive_p2.cpp`
//! (`sc_blend_fillet_executive_p2_eval`). Upstream: `source/SMLib/src/SmFilletExecutive.cpp`,
//! `source/SMLib/inc/SmFilletExecutive.h`.
//!
//! A box block is `P[3] l w h` (`SmApiCreateBox`); its edges are `SmBrep::GetEdges`
//! order, which is the order of unit `heal.stitch_p2`'s export of the same box. Every
//! output starts with the status (`0` = `SM_SUCCESS`, else the `SmStatus`; `-2` =
//! upstream would dereference null, refused before the call; `-4 n_edges` = the box has
//! fewer than [`BOX_EDGES`] edges, which `SmApiCreateBox` gives where the sides
//! collapse at the origin's scale, and no op runs).
//!
//! | [`Op`]        | parameters | outputs |
//! |---------------|------------|---------|
//! | `LawLinear`   | `start end has_map min max` | `0 map_min map_max start end arc_length` (`SmLinearFilletLaw`) |
//! | `LawElliptic` | `start end outer span start_deriv end_deriv has_map min max` | `0 map_min map_max has_curve [n p m cpts*4 knots] has_extended` (elliptical `SmBSplineFilletLaw`) |
//! | `SetParams`   | `has_brep box n e*n nx x*nx nr r*nr nv v*nv has_laws nl (kind [start end])*nl approx ang tan xsect cont thumb` | `status initialized piecewise insertion global trimming classification target_is_brep n_solvers error_code` (`SetFilletParameters`, edge pointers, per-edge parameters) |
//! | `Exec`        | `sub k box` | `status seq_len list_len` (`sub` 0 `SetFilletBrepEdgeMap` with an empty fillet B-rep, 1 `ProcessSmoothEdgeSequences`, 2 `CollectTangentEdgeSequence`, on `k` constant-radius solvers on the box's first edges) |
//! | `TempRadius`  | `vr start end box` | `status [start end during, start end after]` (`SmTemporaryRadiusChange` on a solver of the box's first edge) |
//!
//! [`eval`] accepts only what upstream handles without undefined behaviour: finite values
//! of magnitude at most [`MAX_VALUE`], box sides in `[1e-3, 1e6]`, edge indices of the
//! box, enum codes inside `SmFilletSurfaceGeneratorType` / `SmFilletSolverType`, law
//! kinds `0` (null) and `1` (linear), lists of at most [`MAX_LIST`] entries, and the
//! edge map only without solvers (their fillet geometry, which `CalcFilletGeom` builds,
//! has null rails before that).

//! Direct p1 loader records (`LoadSolver`) contain `scale g1 n (edge mask radial
//! geometry_surfaces)*n m (reserved_zero solver)*m clear_after`. Edge12 has no rails;
//! edge13 is a real closed circular wire. The shim checks the actual rail/vertex
//! state before the native uninitialized `pVerts` branch; defined early errors run.

use crate::{call_with_buffer, FfiError};

/// Largest accepted magnitude of a numeric parameter.
pub const MAX_VALUE: f64 = 1.0e6;
/// Most entries of a type, value, edge or law list.
pub const MAX_LIST: usize = 13;
/// Edges of an `SmApiCreateBox` box.
pub const BOX_EDGES: usize = 12;
/// Largest `SmFilletSurfaceGeneratorType` code (`SM_FSG_BLEND_CURVE`).
pub const MAX_FSG_TYPE: usize = 3;
/// Largest `SmFilletSolverType` code (`SM_FS_CONST_RADIUS_ASSISTED`).
pub const MAX_SOLVER_TYPE: usize = 6;

/// Shim operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum Op {
    LawLinear = 0,
    LawElliptic = 1,
    SetParams = 2,
    Exec = 3,
    TempRadius = 4,
    LoadSolver = 5,
}

mod sys {
    extern "C" {
        pub fn sc_blend_fillet_executive_p2_eval(
            op: i32,
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

crate::reader::unit_reader!();

fn bad(msg: &str) -> FfiError {
    FfiError::InvalidArgument(msg.into())
}

impl Reader<'_> {
    /// A finite value of magnitude at most [`MAX_VALUE`].
    fn value(&mut self) -> Result<f64, FfiError> {
        let v = self.num()?;
        if !v.is_finite() || v.abs() > MAX_VALUE {
            return Err(bad("value not finite or too large"));
        }
        Ok(v)
    }

    /// `P[3] l w h`.
    fn box_block(&mut self) -> Result<(), FfiError> {
        for _ in 0..3 {
            self.value()?;
        }
        for _ in 0..3 {
            let d = self.value()?;
            if !(1e-3..=1e6).contains(&d) {
                return Err(bad("box size out of range"));
            }
        }
        Ok(())
    }

    /// `n x*n` with each `x` an integer in `0..=max`.
    fn codes(&mut self, max: usize) -> Result<usize, FfiError> {
        let n = self.index(MAX_LIST)?;
        for _ in 0..n {
            self.index(max)?;
        }
        Ok(n)
    }
}

fn validate_op(op: Op, rd: &mut Reader<'_>) -> Result<(), FfiError> {
    match op {
        Op::LawLinear => {
            rd.value()?;
            rd.value()?;
            rd.flag()?;
            rd.value()?;
            rd.value()?;
        }
        Op::LawElliptic => {
            for _ in 0..6 {
                rd.value()?;
            }
            rd.flag()?;
            rd.value()?;
            rd.value()?;
        }
        Op::SetParams => {
            rd.flag()?;
            rd.box_block()?;
            let n = rd.index(MAX_LIST)?;
            for _ in 0..n {
                if rd.index(BOX_EDGES)? == BOX_EDGES {
                    return Err(bad("edge index outside the box"));
                }
            }
            rd.codes(MAX_FSG_TYPE)?;
            rd.codes(MAX_SOLVER_TYPE)?;
            let nv = rd.index(MAX_LIST)?;
            for _ in 0..nv {
                rd.value()?;
            }
            rd.flag()?;
            let nl = rd.index(MAX_LIST)?;
            for _ in 0..nl {
                if rd.index(1)? == 1 {
                    rd.value()?;
                    rd.value()?;
                }
            }
            for _ in 0..4 {
                rd.value()?;
            }
            rd.index(3)?;
            rd.value()?;
        }
        Op::Exec => {
            let sub = rd.index(5)?;
            let k = rd.index(BOX_EDGES)?;
            // SetFilletBrepEdgeMap reads the rails of each solver's fillet geometry,
            // which only CalcFilletGeom builds: with solvers fresh from
            // SetFilletParameters upstream dereferences null rails.
            if sub == 0 && k > 0 {
                return Err(bad(
                    "edge map with solvers whose geometry was not calculated",
                ));
            }
            if sub >= 3 && k < 2 {
                return Err(bad("split-edge smooth sequence requires at least 2 solvers"));
            }
            rd.box_block()?;
        }
        Op::LoadSolver => {
            let scale=rd.value()?;
            if [scale, 2.0 * scale, 3.0 * scale]
                .iter()
                .any(|dimension| !(1e-3..=1e6).contains(dimension))
            {
                return Err(bad("loader box dimension out of range"));
            }
            rd.flag()?;
            let count=rd.index(6)?;
            let mut specs=Vec::new();
            for _ in 0..count {
                let edge=rd.index(BOX_EDGES+1)?;
                let mask=rd.index(3)?;
                rd.flag()?;
                rd.flag()?;
                specs.push((edge,mask));
            }
            let loads=rd.index(16)?;
            for _ in 0..loads {
                let executive=rd.index(0)?;
                let solver=rd.index(count)?;
                specs.get(solver).ok_or_else(||bad("loader solver index outside pool"))?;
                let _=executive;
            }
            rd.index(loads)?;
        }
        Op::TempRadius => {
            rd.flag()?;
            rd.value()?;
            rd.value()?;
            rd.box_block()?;
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
        // reads for `op` (validated above: every count, index, enum code and flag the
        // shim passes to upstream); buf is valid for buf.len() writes and the shim
        // honours the capacity; len is valid.
        unsafe {
            sys::sc_blend_fillet_executive_p2_eval(
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

#[cfg(test)]
mod tests {
    use super::{validate, Op};

    #[test]
    fn loader_dimensions_match_the_shared_box_exporter() {
        for scale in [1e-3, 1.0, 1e6 / 3.0] {
            assert!(validate(Op::LoadSolver, &[scale, 0.0, 0.0, 0.0, 0.0]).is_ok());
        }
        for scale in [0.0, 1e-4, 1e6 / 2.0, 1e6, f64::INFINITY, f64::NAN] {
            assert!(validate(Op::LoadSolver, &[scale, 0.0, 0.0, 0.0, 0.0]).is_err());
        }
    }
}
