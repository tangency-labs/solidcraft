//! FFI wrappers for unit `blend.fillet_corner_p3`.
//!
//! Upstream: `source/SMLib/src/SmFilletCorner.cpp`. Two shims:
//!
//! - [`create`] (`harness/cpp/shim/blend_fillet_corner_p3.cpp`,
//!   `sc_blend_fillet_corner_p3_create`): `SmFilletCorner::Create` on a real `SmBrep`. The
//!   input is unit `topology.edgeuse_p1`'s B-rep block (layout in
//!   [`super::topology_edgeuse_p1`]), then `v ns (eu approx tang fsg)..`: the graph index
//!   of the filleted vertex, then per filleted edge use its graph index, its solver's
//!   approximation and tangency tolerances and surface generator kind (`0` the base
//!   `SmFilletSurfaceGenerator`, `1` `SmLinearCrossSectionFSG`, `2`
//!   `SmCircularCrossSectionFSG()`, `3` `SmBlendCurveCrossSectionFSG()`). The executive's
//!   edge-use-to-solver map holds one base solver per filleted edge use. Output: `0`, then
//!   `-2` (refused: the walk around the vertex would dereference null or never close, or
//!   the sector convexity test would dereference null), `-3` (not run: deferred on the
//!   Rust side), or `0` (no corner) / `1 type n_all all.. approx tang bevel blending
//!   chamfer degenerate const_rad adjusted ext_patch setback n_verts n_edges n_surfs g1`
//!   (`g1` the closed Nx1 corner's `m_bG1Case`, `-1` for the other kinds). A B-rep that
//!   cannot be built gives `-1 status`.
//! - [`corner`] (`harness/cpp/shim/blend_fillet_corner_p3_static.inc`, compiled inside
//!   `blend_fillet_geom_p1.cpp`, `sc_blend_fillet_corner_p3_corner`): one corner operation
//!   ([`Op`]) on a corner of a given derived class built on `blend.fillet_geom_p1`'s
//!   pseudo-B-rep model (layout in [`super::blend_fillet_geom_p1`]). After the block:
//!   `kind v_edge v_sub` (the derived class `0..=10` in the order of
//!   `solidcore::blend::fillet_corner::p3::FilletCornerKind`; the filleted vertex as the
//!   start (`0`) / end (`1`) vertex of target wire edge `v_edge`, `-1` for none), the
//!   solver and all edge uses (`has n (edge which)..`: target wire edge, `0` its primary
//!   edge use / `1` the mate; `has = 0` for NULL), the corner's fillet vertices (`n v..`)
//!   and fillet edges (`n e..`), the flags `bevel blending chamfer degenerate ext_patch
//!   adjusted`, `setback approx tang`, and intersection points (`n (edge which t u0 v0 u1
//!   v1 s0 s1)..`: the vertex use of pseudo edge `edge`'s primary edge use (`0`) or mate
//!   (`1`) gets curve parameter `t`, UV positions and as `m_apUserPointer` the surfaces of
//!   target faces `s0`, `s1` (`-1` NULL)), then original faces (`n (edge face)..`:
//!   `SetOriginalFace` of a pseudo edge to a target face). Output: `-2` (refused), `-3` (not run), else
//!   the op status, `n_verts verts.. n_edges edges.. bevel blending chamfer degenerate
//!   const_rad adjusted ext_patch setback n_surfs` and the model export of
//!   `blend.fillet_geom_p1`.
//!
//! The shims refuse exactly the inputs that would make upstream dereference null, read
//! past an array or free the harness's own objects on a path the Rust side does not
//! defer; every other input runs. [`create`] and [`corner`] validate the layout, index
//! ranges and value bounds before the call.

use crate::{call_with_buffer, FfiError};

use super::blend_fillet_geom_p1::{validate_block, Reader as P1Reader, MAX_LIST, MAX_MAGNITUDE};
use super::topology_edgeuse_p1::{self as eu1, brep_block, Reader};

/// The largest number of filleted edge uses of a [`create`] call.
pub const MAX_SOLVER_EUS: usize = 16;

/// The corner operation run by [`corner`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// `SmFilletCorner::CalcCornerGeom`.
    CalcGeom = 0,
    /// The virtual `CreateCornerPatch`.
    Patch = 1,
    /// The virtual `AdjustTopology`.
    AdjustTopology = 2,
    /// The virtual `AdjustTangentCorner`.
    AdjustTangent = 3,
}

mod sys {
    extern "C" {
        pub fn sc_blend_fillet_corner_p3_create(
            path: *const std::ffi::c_char,
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
        pub fn sc_blend_fillet_corner_p3_corner(
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

/// Runs `SmFilletCorner::Create` (layout in the module docs); `path` is the `.smb` file
/// of a file B-rep block (ignored otherwise).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for a malformed input; [`FfiError::Status`] for a shim
/// failure.
pub fn create(path: &str, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    let mut rd = Reader { input, at: 0 };
    brep_block(&mut rd, path)?;
    let block = input.get(..rd.at).ok_or_else(|| bad("input too short"))?;
    let export = eu1::eval(eu1::Op::Export, path, block)?;
    let Some(c) = eu1::counts(&export) else {
        // the B-rep could not be built: the shim reports the same failure
        return Ok(export);
    };
    let n_eu = c.get(6).copied().unwrap_or(0);
    let n_v = c.get(9).copied().unwrap_or(0);
    rd.below(n_v)?;
    let ns = rd.index(MAX_SOLVER_EUS)?;
    if ns == 0 {
        return Err(bad("no filleted edge use (upstream reads (*pSolverEUs)[0])"));
    }
    for _ in 0..ns {
        rd.below(n_eu)?;
        rd.finite()?;
        rd.finite()?;
        rd.index(3)?;
    }
    rd.done()?;
    let cpath = std::ffi::CString::new(path).map_err(|_| bad("path has a NUL byte"))?;
    call_with_buffer(256, |buf, len| {
        // SAFETY: cpath is a valid NUL-terminated string; input is valid for input.len()
        // reads and holds what the shim reads (validated above); buf is valid for
        // buf.len() writes; len is a valid pointer.
        unsafe {
            sys::sc_blend_fillet_corner_p3_create(
                cpath.as_ptr(),
                input.as_ptr(),
                input.len() as i64,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}

/// An edge use list `has n (edge which)..` of target wire edges.
fn eu_list(rd: &mut P1Reader<'_>, nt: usize) -> Result<(), FfiError> {
    rd.flag()?;
    let n = rd.index(MAX_LIST)?;
    for _ in 0..n {
        rd.opt_index(nt)?
            .ok_or_else(|| bad("edge use: target edge -1"))?;
        rd.index(1)?;
    }
    Ok(())
}

/// Validates the input of [`corner`] (layout in the module docs).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for an input upstream could not handle safely.
pub fn validate_corner(input: &[f64]) -> Result<(), FfiError> {
    let mut rd = P1Reader {
        cursor: crate::reader::Cursor::new(input),
        bound: MAX_MAGNITUDE,
    };
    let b = validate_block(&mut rd)?;
    rd.index(10)?; // kind
    let v_edge = rd.opt_index(b.nt)?;
    let v_sub = rd.index(1)?;
    if v_edge.is_none() && v_sub != 0 {
        return Err(bad("vertex sub without edge"));
    }
    eu_list(&mut rd, b.nt)?;
    eu_list(&mut rd, b.nt)?;
    let nv = rd.index(MAX_LIST)?;
    for _ in 0..nv {
        rd.opt_index(b.nv())?.ok_or_else(|| bad("vertex -1"))?;
    }
    let ne = rd.index(MAX_LIST)?;
    for _ in 0..ne {
        rd.opt_index(b.ne())?.ok_or_else(|| bad("edge -1"))?;
    }
    for _ in 0..6 {
        rd.flag()?;
    }
    rd.finites(3)?;
    let nt = rd.index(MAX_LIST)?;
    for _ in 0..nt {
        rd.opt_index(b.ne())?
            .ok_or_else(|| bad("intersection point: edge -1"))?;
        rd.index(1)?;
        rd.finites(5)?;
        for _ in 0..2 {
            // target face, -1 NULL, or 1000 / 1001: a real SmPlane
            let v = rd.num()?;
            let face = v.fract() == 0.0 && v >= 0.0 && v < b.nf as f64;
            if !(v == -1.0 || v == 1000.0 || v == 1001.0 || face) {
                return Err(bad(format!("bad surface selector {v}")));
            }
        }
    }
    let no = rd.index(MAX_LIST)?;
    for _ in 0..no {
        rd.opt_index(b.ne())?.ok_or_else(|| bad("original face: edge -1"))?;
        rd.opt_index(b.nf)?.ok_or_else(|| bad("original face: face -1"))?;
    }
    rd.done()
}

/// Runs the corner operation `op` (layout in the module docs).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for an input upstream could not handle safely;
/// [`FfiError::Status`] for a shim error.
pub fn corner(op: Op, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    validate_corner(input)?;
    call_with_buffer(4096, |buf, len| {
        // SAFETY: input is valid for input.len() reads (validated above); buf is valid
        // for buf.len() writes; len is a valid pointer.
        unsafe {
            sys::sc_blend_fillet_corner_p3_corner(
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
