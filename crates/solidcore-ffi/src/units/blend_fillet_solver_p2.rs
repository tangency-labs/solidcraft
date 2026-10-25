//! FFI wrappers for unit `blend.fillet_solver_p2`.
//!
//! Shim: `harness/cpp/shim/blend_fillet_solver_p2.cpp` (`sc_blend_fillet_solver_p2_eval`).
//! Upstream: `source/SMLib/src/SmFilletSolver.cpp`.
//!
//! Surface, curve and B-rep blocks are those of unit `topology.edgeuse_p1`; entity
//! indices are its export's traversal order. The solver is the shim's `P2FS`, a small
//! rolling-ball solver implementing the virtuals the solves call (layout of `cfg`:
//! `tol extra radius slope target gu gv`, see the shim's header). `surfs` is `base0 base1
//! has0 off0 has1 off1 o0 o1`: offset surface `i` of `base_i` when `has_i`, then the
//! orientations.
//!
//! | [`Op`]      | upstream | parameters | output |
//! |-------------|----------|------------|--------|
//! | `Plane`     | `PointOnPlaneSolve` | `cfg surfs orig[3] normal[3] dom1[4] dom2[4] uv1[2] uv2[2]` | `status off0 off1`, on success `found` and when found the tsect UVs and `extra` user doubles |
//! | `RailEu`    | `RailEdgeuseIntersect` | `cfg surfs block side_eu frac rail ptype` (guess at `frac` of the side edge's interval) | `0 status off0 off1`, on success `found` and when found `param tsect curve_param point_type` |
//! | `RailRail`  | `RailRailIntersect` | `cfgA surfsA cfgB surfsB uv[2] rail other` | `status offA[2] offB[2]`, on success `found` and when found both tsects |
//!
//! `found` is reported only on success: upstream sets it before its last `SER` (the
//! user-double overflow), where callers do not read it.
//! | `Geom`      | `CalcCylinderFilletGeom` / `CalcConeFilletGeom` / `CalcTorusFilletGeom` (`which` 0/1/2) | `which cfg surfs fsg before after factor rails edge eu0 eu1 n` then `n` x `uv0[2] uv1[2] cparam` (`fsg` 0 linear, 1 circular; `rails` `0`, or `1 p[4][3]`: the rail end vertices typed `SM_FV_RAIL_X_EDGEUSE`, processed, at `p`; `edge` `0 block`, `1 a[3] b[3]` (an `SmLine` segment) or `2 radius span_deg` (an `SmCircle` about the z axis through `span_deg`)) | `0 status`, on success `surface rail0 rail1 [ivl0[2] ivl1[2]] center` (intervals for the cone and torus) |
//! | `Misc`      | `GetFilletCorner` (`0 cfg index`), `RecordFilletError` (`1 cfg block eu0`) | | `null jacobian_size`; `0 code status edge_type filleted_ok ns surf0_ok nc curv0_ok ne rail0_ok loc_init [loc[3]] comment` |
//!
//! A curve is `1 t0 t1 P(t0) P(mid) P(t1)` or `-1`; a surface `1 u0 v0 u1 v1` and the
//! 3 x 3 grid of points or `-1`; a B-rep that cannot be built gives `-1 status`.
//!
//! [`eval`] accepts only what upstream handles without undefined behaviour: valid
//! blocks, finite numbers, `extra <= 5`, rails `0`/`1`, point types of
//! `SmIntersectionPointType`, at most [`MAX_TSECTS`] tsect points; `Plane` with both
//! offset surfaces (`*GetSurface(0)` and the solver's `LoadJacobian` dereference them)
//! and `extra <= 4` (beyond, upstream writes past `m_adUserDoubles`, UB-192); `RailEu`
//! with a side edge use of the B-rep, on a file or face B-rep when both offset surfaces
//! are present (`GetOrCreateUVTrimCurve` needs the edge use's face); `Geom` with both
//! offset surfaces, a first edge use (`GetEdgeuse(0)->GetEdge()`), a second one for the
//! torus (its rail-end checks dereference `GetEdgeuse(1)`) and at least one tsect point
//! (`rTsectPnts[0]`); `GetFilletCorner` only above index 1 (below, upstream dereferences
//! the `NULL` executive); `RecordFilletError` with a first edge use.
//!
//! The solver's sizes always agree: its `GetJacobianSize` (`4 + extra`) matches the
//! unknowns its `LoadInitialValues` appends, so every solve's guess has the size of its
//! intervals (`SmLocalSolveNd::SolveIt` indexes the intervals and periodicities by the
//! guess size: a mismatch reads or clamps past them, undefined upstream; the Rust solve
//! refuses it).

use crate::units::topology_edgeuse_p1::{self as eu1, bad, brep_block, surface, Reader};
use crate::{call_with_buffer, FfiError};

/// Largest number of tsect points accepted by [`eval`].
pub const MAX_TSECTS: usize = 16;
/// Largest `extra` (unknowns beyond the two UV points) of the shim's solver.
pub const MAX_EXTRA: usize = 5;
/// Largest `SmIntersectionPointType` value.
const MAX_POINT_TYPE: usize = 7;
/// Shortest line segment and smallest circle radius of a `Geom` edge (shorter ones make
/// `CreateLineSegment` / `CreateCanonical` fail, a status the shim does not report).
const MIN_EDGE: f64 = 1.0e-3;
/// Largest circle radius of a `Geom` edge.
const MAX_RADIUS: f64 = 1.0e3;

/// The upstream routine run by the shim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// `PointOnPlaneSolve`.
    Plane,
    /// `RailEdgeuseIntersect`.
    RailEu,
    /// `RailRailIntersect`.
    RailRail,
    /// `CalcCylinderFilletGeom`, `CalcConeFilletGeom`, `CalcTorusFilletGeom`.
    Geom,
    /// `GetFilletCorner`, `RecordFilletError`.
    Misc,
}

impl Op {
    fn code(self) -> i32 {
        match self {
            Op::Plane => 0,
            Op::RailEu => 1,
            Op::RailRail => 2,
            Op::Geom => 3,
            Op::Misc => 4,
        }
    }
}

mod sys {
    extern "C" {
        pub fn sc_blend_fillet_solver_p2_eval(
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

/// `tol extra radius slope target gu gv`: returns `extra`.
fn solver(rd: &mut Reader<'_>) -> Result<usize, FfiError> {
    rd.finite()?;
    let extra = rd.index(MAX_EXTRA)?;
    rd.finites(5)?;
    Ok(extra)
}

/// `base0 base1 has0 off0 has1 off1 o0 o1`: returns the presence flags.
fn surfaces(rd: &mut Reader<'_>) -> Result<[bool; 2], FfiError> {
    surface(rd)?;
    surface(rd)?;
    let h0 = rd.flag()?;
    rd.finite()?;
    let h1 = rd.flag()?;
    rd.finite()?;
    rd.finites(2)?;
    Ok([h0, h1])
}

/// A B-rep block at the reader's position: its kind and export counts (`None` when the
/// B-rep cannot be built: the shim then reports the failure and reads nothing more).
fn brep(rd: &mut Reader<'_>, path: &str) -> Result<(f64, Option<[usize; 10]>), FfiError> {
    let start = rd.at;
    let kind = rd.input.get(start).copied().unwrap_or(-1.0);
    brep_block(rd, path)?;
    let block = rd
        .input
        .get(start..rd.at)
        .ok_or_else(|| bad("input too short"))?;
    let export = eu1::eval(eu1::Op::Export, path, block)?;
    Ok((kind, eu1::counts(&export)))
}

/// Validates `input` for `op`; `Ok(false)` when a B-rep cannot be built (the rest of the
/// input is not read by the shim).
fn validate(op: Op, path: &str, input: &[f64]) -> Result<bool, FfiError> {
    let mut rd = Reader { input, at: 0 };
    match op {
        Op::Plane => {
            let extra = solver(&mut rd)?;
            let has = surfaces(&mut rd)?;
            if !(has[0] && has[1]) {
                return Err(bad("plane: both offset surfaces are dereferenced"));
            }
            if extra > 4 {
                return Err(bad(
                    "plane: more user doubles than m_adUserDoubles (UB-192)",
                ));
            }
            rd.finites(18)?;
        }
        Op::RailEu => {
            solver(&mut rd)?;
            let has = surfaces(&mut rd)?;
            let (kind, counts) = brep(&mut rd, path)?;
            let Some(counts) = counts else {
                return Ok(false);
            };
            if has[0] && has[1] && kind != 0.0 && kind != 2.0 {
                return Err(bad("rail-eu: the side edge use needs a face"));
            }
            rd.below(counts[6])?;
            rd.finite()?;
            rd.index(1)?;
            rd.index(MAX_POINT_TYPE)?;
        }
        Op::RailRail => {
            for _ in 0..2 {
                solver(&mut rd)?;
                surfaces(&mut rd)?;
            }
            rd.finites(2)?;
            rd.index(1)?;
            rd.index(1)?;
        }
        Op::Geom => {
            let which = rd.index(2)?;
            solver(&mut rd)?;
            let has = surfaces(&mut rd)?;
            if !(has[0] && has[1]) {
                return Err(bad("geom: both offset surfaces are dereferenced"));
            }
            rd.index(1)?;
            rd.flag()?;
            rd.flag()?;
            rd.finite()?;
            // the rail end vertices: 1 = typed RAIL_X_EDGEUSE, processed, at 4 points
            if rd.index(1)? == 1 {
                rd.finites(12)?;
            }
            // the filleted edge: a B-rep block, an SmLine segment or an SmCircle arc
            let edge_kind = rd.index(2)?;
            let n_edgeuses = match edge_kind {
                0 => {
                    let (_, counts) = brep(&mut rd, path)?;
                    let Some(counts) = counts else {
                        return Ok(false);
                    };
                    counts[6]
                }
                1 => {
                    let ends = rd.finites(6)?;
                    let (a, b) = ends.split_at(3);
                    let d: f64 = a.iter().zip(b).map(|(x, y)| (x - y) * (x - y)).sum();
                    if d.sqrt() <= MIN_EDGE {
                        return Err(bad("geom: line segment shorter than MIN_EDGE"));
                    }
                    2
                }
                _ => {
                    let r = rd.finite()?;
                    let span = rd.finite()?;
                    if !(MIN_EDGE..=MAX_RADIUS).contains(&r) || !(1.0..=360.0).contains(&span) {
                        return Err(bad("geom: circle radius or span out of range"));
                    }
                    2
                }
            };
            rd.below(n_edgeuses)?;
            if which == 2 {
                rd.below(n_edgeuses)?;
            } else {
                rd.below_or_none(n_edgeuses)?;
            }
            let n = rd.index(MAX_TSECTS)?;
            if n == 0 {
                return Err(bad("geom: rTsectPnts[0] is read"));
            }
            rd.finites(5 * n)?;
        }
        Op::Misc => {
            let which = rd.index(1)?;
            solver(&mut rd)?;
            if which == 0 {
                if rd.index(1000)? < 2 {
                    return Err(bad("corner: the NULL executive is dereferenced"));
                }
            } else {
                let (_, counts) = brep(&mut rd, path)?;
                let Some(counts) = counts else {
                    return Ok(false);
                };
                rd.below(counts[6])?;
            }
        }
    }
    rd.done()?;
    Ok(true)
}

/// Runs `op` on `input` (layouts in the module docs; `path` is the `.smb` file of a file
/// B-rep block, else empty).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for input the shim must not see; [`FfiError::Status`]
/// for a shim failure.
pub fn eval(op: Op, path: &str, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    validate(op, path, input)?;
    let c_path = std::ffi::CString::new(path).map_err(|_| bad("path with NUL"))?;
    call_with_buffer(4096, |buf, len| {
        // SAFETY: c_path is a valid NUL-terminated string and input is valid for
        // input.len() reads, holding what the shim reads for `op` (validated above:
        // well-formed blocks, indices inside the B-rep's export, the counts and flags the
        // shim passes to upstream; only a leading B-rep block is read when the B-rep
        // cannot be built); buf is valid for buf.len() writes and the shim honours the
        // capacity; len is valid.
        unsafe {
            sys::sc_blend_fillet_solver_p2_eval(
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
