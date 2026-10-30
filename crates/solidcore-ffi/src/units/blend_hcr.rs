//! Safe wrapper for `blend.hcr`; shim: `harness/cpp/shim/blend_hcr.cpp`.
//!
//! Input, in order:
//! - the B-rep block of unit `topology.vertexuse` (validated by building it through
//!   [`fixture::eval`]);
//! - `extra`: `0`, or `1 curve t0 t1`: a wire edge added to that B-rep with
//!   `CreateWireEdgeFromCurve` (the curve block of a `topology.vertexuse` wire block,
//!   validated as one);
//! - the view: `0 V[3]` (`SmHCR(.., crViewVector, ..)`) or `1 O[3] Z[3] Xref[3]` (the
//!   placement `SetSTEPCanonical(O, Z, Xref)`), then `approx_tol ang_tol seams smooth`;
//! - for ops other than 8: `n_sil sil[..] n_back back[..]` (scene edge indices seeded as
//!   silhouette and back-facing edges) and the op arguments:
//!   - 0 construction state, 6 `GetSilhouetteEdges`: none;
//!   - 1 `IsSilhouetteEdge`, 2 `IsBackFacingEdge`: `edge`;
//!   - 3 `IntersectCurveWithSilhouettes`, 4 `ComputeCurveVisibility`: `edge fa fb
//!     start_hint end_hint` (the classified interval is the edge interval evaluated at
//!     `0 <= fa, fb <= 1` in either order; any hint in the `int64_t` range);
//!   - 5 `ComputeEdgeVisibility`: `edge vertex(-1 none) visible premark`;
//!   - 7 `IsPointVisible`: `P[3]`;
//!   - 9 `ComputeGlobalVisibility`, 10 `ComputeSilhouettes`: none, and no seeds.
//!
//! Op 8 constructs only; its output gives the scene that [`validate`] checks the seeds
//! and op arguments against. Output: `status record_len record n_e curve_type[n_e]
//! scene_box(6) scene_export` (status `-2`: the upstream path is undefined and was not
//! run). Rejected: a view vector whose squared length is below `SM_EFF_ZERO_SQ` or not
//! finite (UB-169: upstream builds the view transform from uninitialised vectors), entity
//! indices out of range, seeded silhouette edges without curve, non-finite numbers,
//! values above [`MAX_SCENE_MAGNITUDE`] (the ray tracer's grid walk does not end), more
//! than 64 seeds.
//!
//! Ops 9 and 10 run the whole silhouette and visibility computation on solid bodies,
//! each in a fresh process ([`crate::isolate::fresh_process`]), so that a crash in the
//! C++ fails only its own case. Op 9 outputs status `-2` when a scene edge, after the
//! silhouettes of a second `SmHCR` of the same B-rep, reaches UB-170 (more silhouette
//! vertices than edges in its curve's box). Op 10's record is the silhouette edges
//! (`GetSilhouetteEdges` after `ComputeSilhouettes`), each as its construction-graph
//! edge index (`-1` for a new edge), interval and 5 `status x y z` points of its curve,
//! sorted.
use super::topology_vertexuse::{self as fixture, Op};
use crate::{call_with_buffer, FfiError};
use std::ffi::CString;

mod sys {
    extern "C" {
        pub fn sc_blend_hcr_eval(
            op: i32,
            path: *const std::ffi::c_char,
            input: *const f64,
            n: i64,
            out: *mut f64,
            cap: i64,
            len: *mut i64,
        ) -> i32;
    }
}

fn bad(msg: &str) -> FfiError {
    FfiError::InvalidArgument(format!("blend.hcr: {msg}"))
}

struct Reader<'a> {
    v: &'a [f64],
    at: usize,
}

impl Reader<'_> {
    fn num(&mut self) -> Result<f64, FfiError> {
        let x = *self.v.get(self.at).ok_or_else(|| bad("short input"))?;
        self.at += 1;
        if !x.is_finite() {
            return Err(bad("non-finite number"));
        }
        Ok(x)
    }
    fn count(&mut self, max: f64) -> Result<usize, FfiError> {
        let n = self.num()?;
        if n.fract() != 0.0 || !(0.0..=max).contains(&n) {
            return Err(bad("count out of range"));
        }
        Ok(n as usize)
    }
    fn index(&mut self, n: usize, allow_none: bool) -> Result<i64, FfiError> {
        let x = self.num()?;
        if x.fract() != 0.0 || x < if allow_none { -1.0 } else { 0.0 } || x >= n as f64 {
            return Err(bad("index out of range"));
        }
        Ok(x as i64)
    }
    fn flag(&mut self) -> Result<(), FfiError> {
        let x = self.num()?;
        if x != 0.0 && x != 1.0 {
            return Err(bad("flag"));
        }
        Ok(())
    }
}

/// A parsed shim output.
#[derive(Debug, Clone, Copy)]
pub struct SceneOut<'a> {
    /// The op's status (`-2`: undefined upstream path, not run).
    pub status: f64,
    /// The op's record.
    pub record: &'a [f64],
    /// `GetCurve()->GetType()` of each scene edge, `-1` for none.
    pub curve_types: &'a [f64],
    /// The scene's `CalculateBoundingBox`: min, max.
    pub scene_box: [f64; 6],
    /// The scene export (the `topology.vertexuse` layout).
    pub export: &'a [f64],
}

/// Splits a shim output.
#[must_use]
pub fn split(v: &[f64]) -> Option<SceneOut<'_>> {
    let status = *v.first()?;
    let len = usize::try_from(*v.get(1)? as i64).ok()?;
    let record = v.get(2..2 + len)?;
    let n_e = usize::try_from(*v.get(2 + len)? as i64).ok()?;
    let at = 3 + len;
    let curve_types = v.get(at..at + n_e)?;
    let b = v.get(at + n_e..at + n_e + 6)?;
    let scene_box = [*b.first()?, *b.get(1)?, *b.get(2)?, *b.get(3)?, *b.get(4)?, *b.get(5)?];
    Some(SceneOut {
        status,
        record,
        curve_types,
        scene_box,
        export: v.get(at + n_e + 6..)?,
    })
}

/// Validates the `extra` and view part (`hcr`, up to `smooth`).
fn validate_hcr(hcr: &[f64]) -> Result<(), FfiError> {
    let mut rd = Reader { v: hcr, at: 0 };
    let extra = rd.num()?;
    if extra == 1.0 {
        // `dim n p m cpts(4 (n + 1)) knots(m + 1)`, then t0 t1
        let head = hcr.get(rd.at..rd.at + 4).ok_or_else(|| bad("short extra curve"))?;
        let (n, m) = match (head.get(1), head.get(3)) {
            (Some(&n), Some(&m)) if n >= 0.0 && m >= 0.0 && n < 1e4 && m < 1e4 => (n as usize, m as usize),
            _ => return Err(bad("extra curve sizes")),
        };
        let len = 4 + 4 * (n + 1) + (m + 1) + 2;
        let curve = hcr.get(rd.at..rd.at + len).ok_or_else(|| bad("short extra curve"))?;
        let mut block = vec![1.0];
        block.extend_from_slice(curve);
        block.extend([0.0, 0.0]);
        fixture::eval(Op::Export, "", &block)?;
        rd.at += len;
    } else if extra != 0.0 {
        return Err(bad("extra flag"));
    }
    let mode = rd.num()?;
    let (x, y, z) = (rd.num()?, rd.num()?, rd.num()?);
    if mode == 0.0 {
        // UB-169: SmVector3d::Unitize fails below SM_EFF_ZERO_SQ, and an infinite
        // squared length unitizes to zero, for which no perpendicular is found
        let l = x * x + y * y + z * z;
        if !(l >= 1.0e-24 && l.is_finite()) {
            return Err(bad("UB-169: view vector cannot be unitized"));
        }
    } else if mode == 1.0 {
        for _ in 0..6 {
            rd.num()?;
        }
    } else {
        return Err(bad("view mode"));
    }
    rd.num()?;
    rd.num()?;
    rd.flag()?;
    rd.flag()?;
    if rd.at != hcr.len() {
        return Err(bad("trailing view input"));
    }
    Ok(())
}

/// Validates the seeds and arguments of op `op` (`ops`) against the scene of an op-8
/// output (`scene`): what both sides check before running.
pub fn validate(op: u8, scene: &[f64], ops: &[f64]) -> Result<(), FfiError> {
    if op > 10 || op == 8 {
        return Err(bad("op"));
    }
    if op == 7 {
        // the point `IsPointVisible` fires its ray from (hints may be any `int64_t`)
        check_magnitudes(ops)?;
    }
    let s = split(scene).ok_or_else(|| bad("bad scene output"))?;
    let c = fixture::counts(s.export).ok_or_else(|| bad("scene export"))?;
    let (n_e, n_v) = (c[7], c[9]);
    let mut rd = Reader { v: ops, at: 0 };
    let n_sil = rd.count(64.0)?;
    for _ in 0..n_sil {
        let e = rd.index(n_e, false)?;
        if s.curve_types.get(e as usize).is_none_or(|&t| t < 0.0) {
            return Err(bad("seeded silhouette edge without curve"));
        }
    }
    let n_back = rd.count(64.0)?;
    for _ in 0..n_back {
        rd.index(n_e, false)?;
    }
    if op >= 9 && n_sil + n_back > 0 {
        return Err(bad("seeds for a whole-scene op"));
    }
    match op {
        1 | 2 => {
            rd.index(n_e, false)?;
        }
        3 | 4 => {
            rd.index(n_e, false)?;
            let (fa, fb) = (rd.num()?, rd.num()?);
            // `SmExtent1d(Evaluate(fa), Evaluate(fb))` swaps an inverted pair (defined for
            // `fa >= fb`); fractions outside [0, 1] evaluate the curve off its edge interval
            if !(0.0..=1.0).contains(&fa) || !(0.0..=1.0).contains(&fb) {
                return Err(bad("interval fractions"));
            }
            // `(ULONG)(int64_t)hint`: C++ reads 0 as "no hint", 1 as visible and any other
            // value as invisible; only a hint outside the `int64_t` range is undefined
            for _ in 0..2 {
                let h = rd.num()?;
                if !(-9.223_372_036_854_775_808e18..9.223_372_036_854_775_808e18).contains(&h) {
                    return Err(bad("hint"));
                }
            }
        }
        5 => {
            rd.index(n_e, false)?;
            rd.index(n_v, true)?;
            rd.flag()?;
            rd.flag()?;
        }
        7 => {
            for _ in 0..3 {
                rd.num()?;
            }
        }
        _ => {}
    }
    if rd.at != ops.len() {
        return Err(bad("trailing op input"));
    }
    Ok(())
}

fn call(op: u8, path: &str, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    let cpath = CString::new(path).map_err(|_| bad("NUL in path"))?;
    call_with_buffer(1 << 16, |out, len| {
        // SAFETY: the B-rep block, the extra curve and the view are validated by the
        // callers (`scene`, and `validate` for the seeds and op arguments against the
        // scene's counts); the shim rejects seeded edges without curve and skips
        // (status -2) the undefined upstream paths. The buffers and path live for the
        // call and no C++ pointer escapes.
        unsafe {
            sys::sc_blend_hcr_eval(
                i32::from(op),
                cpath.as_ptr(),
                input.as_ptr(),
                input.len() as i64,
                out.as_mut_ptr(),
                out.len() as i64,
                &raw mut *len,
            )
        }
    })
}

/// Largest magnitude of a scene value (B-rep block, extra wire, view, op 7's point; the
/// `±f64::MAX` markers aside). With a coordinate near `1e20` in a scene of size `2e4`
/// (between `1e18`, which returns, and `1e20`), `SmHCR::IsPointVisible` ->
/// `SmRayTracer::FireRay` -> `SmGrid::Step3DDDA` never leaves its voxel walk: the
/// grid's voxels are so large that the ray's steps no longer move it.
pub const MAX_SCENE_MAGNITUDE: f64 = 1.0e15;

fn check_magnitudes(v: &[f64]) -> Result<(), FfiError> {
    if v.iter().any(|x| {
        let a = x.abs();
        a > MAX_SCENE_MAGNITUDE && a < f64::MAX
    }) {
        return Err(bad("scene value above 1e15 (grid walk does not end)"));
    }
    Ok(())
}

/// Op 8: builds the B-rep block `brep` (file `path` for a file block), adds the extra
/// wire and constructs the HCR from the view part `hcr`; returns the scene output.
pub fn scene(path: &str, brep: &[f64], hcr: &[f64]) -> Result<Vec<f64>, FfiError> {
    check_magnitudes(brep)?;
    check_magnitudes(hcr)?;
    let ex = fixture::eval(Op::Export, path, brep)?;
    fixture::counts(&ex).ok_or_else(|| bad("fixture rejected"))?;
    validate_hcr(hcr)?;
    let mut input = brep.to_vec();
    input.extend_from_slice(hcr);
    call(8, path, &input)
}

/// Runs op `op` (0..=7, 9, 10) with the seeds and op arguments `ops`, after [`scene`]
/// and [`validate`]; ops 9 and 10 in a fresh process.
pub fn eval(op: u8, path: &str, brep: &[f64], hcr: &[f64], ops: &[f64]) -> Result<Vec<f64>, FfiError> {
    let s = scene(path, brep, hcr)?;
    validate(op, &s, ops)?;
    let mut input = brep.to_vec();
    input.extend_from_slice(hcr);
    input.extend_from_slice(ops);
    if op >= 9 {
        return crate::isolate::fresh_process(|| call(op, path, &input));
    }
    call(op, path, &input)
}
